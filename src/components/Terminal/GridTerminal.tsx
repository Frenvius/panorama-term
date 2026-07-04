import React from 'react';

import { getSetting } from '~/adapter/settings/settings.client';
import { scheduleConnect } from '~/usecase/util/connectScheduler';
import { TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
import { keyToBytes } from '~/usecase/util/terminalKeys';
import { orderSel, selectText, lineSelection, wordSelection } from '~/usecase/util/terminalSelection';
import { readClipboard, writeClipboard, hasClipboardImage } from '~/adapter/clipboard/clipboard.client';
import { sendPtyInput, sendPtyScroll, sendPtyResize, openPtyConnection } from '~/adapter/pty/pty.client';

import type { GridFrame } from '~/domain/interfaces/pty.interface';
import type { Cell, Selection } from '~/usecase/util/terminalSelection';

import styles from './styles.module.scss';

interface GridTerminalProps {
  tileId: string;
  cwd?: string;
  cols: number;
  rows: number;
  active: boolean;
  visible: boolean;
  k: number;
}

const FONT = 12;
const CELL_H = 15;
const PAINT_MS = 60;
const CLICK_MS = 400;
const BG = '#0b0e14';

let cellW = 7.23;
let fontReady = false;
const measureCell = () => {
  const c = document.createElement('canvas').getContext('2d');
  if (!c) return;
  c.font = `${FONT}px Hack, monospace`;
  const w = c.measureText('M').width;
  if (w > 0) cellW = w;
};

const ensureFont = (): Promise<void> => {
  if (fontReady) return Promise.resolve();
  return Promise.all([
    document.fonts.load(`${FONT}px Hack`),
    document.fonts.load(`bold ${FONT}px Hack`)
  ]).then(() => {
    fontReady = true;
    measureCell();
  });
};

const hex = (v: number): string => '#' + (v & 0xffffff).toString(16).padStart(6, '0');

const GridTerminal = ({ tileId, cwd, cols, rows, active, visible, k }: GridTerminalProps) => {
  const canvasRef = React.useRef<HTMLCanvasElement>(null);
  const wsRef = React.useRef<WebSocket | null>(null);
  const frameRef = React.useRef<GridFrame | null>(null);
  const scrollRef = React.useRef(0);
  const dirtyRef = React.useRef(true);
  const blinkRef = React.useRef(true);
  const selRef = React.useRef<Selection | null>(null);
  const selectingRef = React.useRef(false);
  const clickRef = React.useRef({ t: 0, row: -1, col: -1, count: 0 });
  const activeRef = React.useRef(active);
  const visibleRef = React.useRef(visible);
  const kRef = React.useRef(k);
  const colsRef = React.useRef(cols);
  const rowsRef = React.useRef(rows);
  activeRef.current = active;
  visibleRef.current = visible;
  kRef.current = k;
  colsRef.current = cols;
  rowsRef.current = rows;

  const draw = React.useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    const frame = frameRef.current;
    if (!canvas || !ctx || !frame) return;
    if (!visibleRef.current) {
      if (canvas.width !== 1) {
        canvas.width = 1;
        canvas.height = 1;
        canvas.style.width = '0px';
        canvas.style.height = '0px';
      }
      return;
    }
    const nCols = frame.cols;
    const nRows = frame.rows;
    const w = nCols * cellW;
    const h = nRows * CELL_H;
    const scale = (window.devicePixelRatio || 1) * kRef.current;
    const bw = Math.ceil(w * scale);
    const bh = Math.ceil(h * scale);
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width = bw;
      canvas.height = bh;
      canvas.style.width = `${w}px`;
      canvas.style.height = `${h}px`;
    }
    ctx.setTransform(scale, 0, 0, scale, 0, 0);
    ctx.fillStyle = BG;
    ctx.fillRect(0, 0, w, h);
    ctx.textBaseline = 'top';
    const snap = (v: number) => Math.round(v * scale) / scale;

    const lines = frame.lines;
    const attrs = frame.attrs;
    const yOff = (CELL_H - FONT) / 2;
    for (let r = 0; r < nRows; r++) {
      const line = lines[r] ?? '';
      const cells = Array.from(line);
      for (let c = 0; c < nCols; c++) {
        const i = (r * nCols + c) * 2;
        const w0 = attrs[i] ?? 0;
        const w1 = attrs[i + 1] ?? 0;
        if (w1 & 0x80000000) {
          ctx.fillStyle = hex(w1);
          ctx.fillRect(c * cellW, r * CELL_H, cellW + 0.5, CELL_H);
        }
        const ch = cells[c];
        if (ch && ch !== ' ') {
          ctx.font = `${w0 & (1 << 24) ? 'bold ' : ''}${FONT}px Hack, monospace`;
          ctx.fillStyle = hex(w0);
          const cp = ch.codePointAt(0) ?? 0;
          const box = cp >= 0x2500 && cp <= 0x259f;
          ctx.fillText(ch, box ? c * cellW : snap(c * cellW), box ? r * CELL_H + yOff : snap(r * CELL_H + yOff));
        }
      }
    }

    const sel = selRef.current;
    if (sel) {
      const { s, e } = orderSel(sel);
      ctx.fillStyle = 'rgba(74,144,217,0.35)';
      for (let r = s.row; r <= e.row; r++) {
        if (r < 0 || r >= nRows) continue;
        const c0 = r === s.row ? s.col : 0;
        const c1 = r === e.row ? e.col : nCols - 1;
        ctx.fillRect(c0 * cellW, r * CELL_H, (c1 - c0 + 1) * cellW, CELL_H);
      }
    }

    if (activeRef.current && blinkRef.current && !frame.cursorHidden) {
      ctx.fillStyle = 'rgba(199,208,224,0.65)';
      ctx.fillRect(frame.cursorCol * cellW, frame.cursorRow * CELL_H, cellW, CELL_H);
    }
  }, []);

  React.useEffect(() => {
    let disposed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const target = getSetting(TERMINAL_TARGET_KEY, 'auto');
    measureCell();
    ensureFont().then(() => {
      if (!disposed) dirtyRef.current = true;
    });
    const paint = setInterval(() => {
      if (!dirtyRef.current) return;
      dirtyRef.current = false;
      draw();
    }, PAINT_MS);

    const connect = () => {
      if (disposed) return;
      const ws = openPtyConnection(
        { tileId, cwd, cols: colsRef.current, rows: rowsRef.current, target },
        {
          onGrid: (frame) => {
            frameRef.current = frame;
            scrollRef.current = frame.offset;
            dirtyRef.current = true;
          },
          onExit: () => {},
          onReady: () => {}
        }
      );
      wsRef.current = ws;
      ws.onclose = () => {
        if (!disposed) retry = setTimeout(() => scheduleConnect(connect, visibleRef.current ? 2 : 1), 2000);
      };
    };
    scheduleConnect(connect, visibleRef.current ? 2 : 0);

    return () => {
      disposed = true;
      clearInterval(paint);
      clearTimeout(retry);
      wsRef.current?.close();
      wsRef.current = null;
      frameRef.current = null;
    };
  }, [tileId, cwd, draw]);

  React.useEffect(() => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) sendPtyResize(ws, cols, rows);
  }, [cols, rows]);

  React.useEffect(() => {
    dirtyRef.current = true;
  }, [k, visible]);

  React.useEffect(() => {
    blinkRef.current = true;
    dirtyRef.current = true;
    if (!active) return;
    canvasRef.current?.focus();
    const id = setInterval(() => {
      blinkRef.current = !blinkRef.current;
      dirtyRef.current = true;
    }, 530);
    return () => clearInterval(id);
  }, [active]);

  const cellFromEvent = (e: React.PointerEvent): Cell | null => {
    const canvas = canvasRef.current;
    const frame = frameRef.current;
    if (!canvas || !frame) return null;
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    const col = Math.floor(((e.clientX - rect.left) / rect.width) * frame.cols);
    const row = Math.floor(((e.clientY - rect.top) / rect.height) * frame.rows);
    return { col: Math.max(0, Math.min(frame.cols - 1, col)), row: Math.max(0, Math.min(frame.rows - 1, row)) };
  };

  const selectedText = (): string => {
    const sel = selRef.current;
    const frame = frameRef.current;
    if (!sel || !frame) return '';
    return selectText(frame.lines, frame.cols, sel);
  };

  const clearSelection = () => {
    if (!selRef.current) return;
    selRef.current = null;
    dirtyRef.current = true;
  };

  const copySelection = () => {
    const text = selectedText();
    if (text) writeClipboard(text);
  };

  const pasteText = (ws: WebSocket) => {
    void readClipboard().then((t) => {
      if (t) sendPtyInput(ws, t.replace(/\r\n/g, '\r').replace(/\n/g, '\r'));
    });
  };

  const paste = () => {
    const ws = wsRef.current;
    if (!ws) return;
    void hasClipboardImage().then((hasImage) => {
      if (hasImage) sendPtyInput(ws, '\x1bv');
      else pasteText(ws);
    });
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || !activeRef.current) return;
    const cell = cellFromEvent(e);
    if (!cell) return;
    e.stopPropagation();

    const frame = frameRef.current;
    const now = performance.now();
    const l = clickRef.current;
    const near = cell.row === l.row && Math.abs(cell.col - l.col) <= 1 && now - l.t < CLICK_MS;
    const count = near ? l.count + 1 : 1;
    clickRef.current = { t: now, row: cell.row, col: cell.col, count };

    if (count >= 3 && frame) {
      selectingRef.current = false;
      selRef.current = lineSelection(cell.row, frame.cols);
      dirtyRef.current = true;
      return;
    }
    if (count === 2 && frame) {
      selectingRef.current = false;
      selRef.current = wordSelection(frame.lines[cell.row] ?? '', cell.row, cell.col);
      dirtyRef.current = true;
      return;
    }

    canvasRef.current?.setPointerCapture(e.pointerId);
    selectingRef.current = true;
    selRef.current = { a: cell, b: cell };
    dirtyRef.current = true;
  };

  const onPointerMove = (e: React.PointerEvent) => {
    if (!selectingRef.current || !selRef.current) return;
    const cell = cellFromEvent(e);
    if (!cell) return;
    selRef.current = { a: selRef.current.a, b: cell };
    dirtyRef.current = true;
  };

  const onPointerUp = (e: React.PointerEvent) => {
    if (!selectingRef.current) return;
    selectingRef.current = false;
    canvasRef.current?.releasePointerCapture(e.pointerId);
    const s = selRef.current;
    if (s && s.a.row === s.b.row && s.a.col === s.b.col) selRef.current = null;
    dirtyRef.current = true;
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const ws = wsRef.current;
    if (!ws) return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.toLowerCase();
    if (mod && e.shiftKey && key === 'c') {
      e.preventDefault();
      copySelection();
      return;
    }
    if (mod && key === 'v') {
      e.preventDefault();
      paste();
      return;
    }
    if (mod && !e.shiftKey && key === 'c' && selRef.current) {
      e.preventDefault();
      copySelection();
      clearSelection();
      return;
    }
    const bytes = keyToBytes(e);
    if (bytes === null) return;
    e.preventDefault();
    blinkRef.current = true;
    clearSelection();
    sendPtyInput(ws, bytes);
  };

  const onWheel = (e: React.WheelEvent) => {
    if (!activeRef.current) return;
    const ws = wsRef.current;
    if (!ws) return;
    e.stopPropagation();
    const next = Math.max(0, scrollRef.current + (e.deltaY < 0 ? 3 : -3));
    if (next === scrollRef.current) return;
    scrollRef.current = next;
    clearSelection();
    sendPtyScroll(ws, next);
  };

  return (
    <canvas
      ref={canvasRef}
      tabIndex={-1}
      onWheel={onWheel}
      onKeyDown={onKeyDown}
      onPointerUp={onPointerUp}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerCancel={onPointerUp}
      className={styles.wasm}
    />
  );
};

export default GridTerminal;
