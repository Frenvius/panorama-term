import React from 'react';

import AgentBar from '~/components/Terminal/AgentBar';
import { termTheme, THEME_EVENT } from '~/usecase/util/theme';
import { getSetting } from '~/adapter/settings/settings.client';
import { scheduleConnect } from '~/usecase/util/connectScheduler';
import { TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
import { keyToBytes } from '~/usecase/util/terminalKeys';
import { notifyClaude, clearNotify } from '~/components/commons/Notifications/bridge';
import { openUrl } from '~/adapter/shell/shell.client';
import { urlSpanAt, orderSel, selectText, lineSelection, wordSelection } from '~/usecase/util/terminalSelection';
import { readClipboard, writeClipboard, hasClipboardImage } from '~/adapter/clipboard/clipboard.client';
import { sendPtyKill, sendPtyMouse, sendPtyFocus, sendPtyInput, sendPtyScroll, sendPtyResize, openPtyConnection } from '~/adapter/pty/pty.client';

import type { GridFrame, ClaudeState } from '~/domain/interfaces/pty.interface';
import type { Cell, Selection, UrlSpan } from '~/usecase/util/terminalSelection';

import styles from './styles.module.scss';

interface GridTerminalProps {
  tileId: string;
  cwd?: string;
  cols: number;
  rows: number;
  active: boolean;
  visible: boolean;
  k: number;
  restartKey: number;
  onCwd: (id: string, cwd: string, branch?: string) => void;
  onOscTitle: (id: string, title: string) => void;
  onClaudeActive?: (active: boolean) => void;
  onClaudeStatus?: (status: string) => void;
  onClaudeDiff?: (added: number, removed: number) => void;
  onProgress?: (state: number, pct: number) => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}

const FONT = 12;
const CELL_H = 15;
const WHEEL_LINE_PX = 100 / 3;
const PAINT_MS = 60;
const CLICK_MS = 400;
const NOTIFY_IDLE_GRACE_MS = 10000;
const DEFAULT_FG = 0xc7d0e0;
const QUAD = [0b0010, 0b0001, 0b1000, 0b1011, 0b1001, 0b1110, 0b1101, 0b0100, 0b0110, 0b0111];

let cellW = 7.23;
let fontReady = false;

const devicePx = (v: number): number => {
  const dpr = window.devicePixelRatio || 1;
  return Math.round(v * dpr) / dpr;
};

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
const fgOf = (w0: number): string => {
  const v = w0 & 0xffffff;
  if (v === DEFAULT_FG) return termTheme.fg;
  return termTheme.ansi?.get(v) ?? hex(v);
};

const GridTerminal = ({ tileId, cwd, cols, rows, active, visible, k, restartKey, onCwd, onOscTitle, onClaudeActive, onClaudeStatus, onClaudeDiff, onProgress, onContextMenu }: GridTerminalProps) => {
  const canvasRef = React.useRef<HTMLCanvasElement>(null);
  const wsRef = React.useRef<WebSocket | null>(null);
  const frameRef = React.useRef<GridFrame | null>(null);
  const claudeRef = React.useRef<ClaudeState | null>(null);
  const statusRef = React.useRef<string | undefined>(undefined);
  const dirtyRef = React.useRef(true);
  const blinkRef = React.useRef(true);
  const selRef = React.useRef<Selection | null>(null);
  const hoverRef = React.useRef<UrlSpan | null>(null);
  const selectingRef = React.useRef(false);
  const clickRef = React.useRef({ t: 0, row: -1, col: -1, count: 0 });
  const mouseFwdRef = React.useRef(false);
  const mouseBtnRef = React.useRef(0);
  const lastFwdRef = React.useRef({ row: -1, col: -1 });
  const wheelAccRef = React.useRef(0);
  const pendingResumeRef = React.useRef(true);
  const prevKRef = React.useRef(k);
  const maskRef = React.useRef(false);
  const prevDimsRef = React.useRef({ cols, rows });
  const settleRef = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const activeRef = React.useRef(active);
  const visibleRef = React.useRef(visible);
  const kRef = React.useRef(k);
  const colsRef = React.useRef(cols);
  const rowsRef = React.useRef(rows);
  const onCwdRef = React.useRef(onCwd);
  const onOscTitleRef = React.useRef(onOscTitle);
  const onClaudeStatusRef = React.useRef(onClaudeStatus);
  const onClaudeDiffRef = React.useRef(onClaudeDiff);
  const onProgressRef = React.useRef(onProgress);
  const agentEventsRef = React.useRef(false);
  const lastAgentEventRef = React.useRef(0);
  const lastNotifyRef = React.useRef(0);
  activeRef.current = active;
  visibleRef.current = visible;
  kRef.current = k;
  colsRef.current = cols;
  rowsRef.current = rows;
  onCwdRef.current = onCwd;
  onOscTitleRef.current = onOscTitle;
  onClaudeStatusRef.current = onClaudeStatus;
  onClaudeDiffRef.current = onClaudeDiff;
  onProgressRef.current = onProgress;

  const isWatching = React.useCallback((): boolean => {
    const rect = canvasRef.current?.getBoundingClientRect();
    let inView = false;
    if (rect && rect.width > 0 && rect.height > 0) {
      const visW = Math.min(rect.right, window.innerWidth) - Math.max(rect.left, 0);
      const visH = Math.min(rect.bottom, window.innerHeight) - Math.max(rect.top, 0);
      const visArea = Math.max(0, visW) * Math.max(0, visH);
      inView = visArea / (rect.width * rect.height) >= 0.5;
    }
    const onScreen = !document.hidden && document.hasFocus();
    return activeRef.current && inView && onScreen;
  }, []);

  const draw = React.useCallback(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext('2d');
    const frame = frameRef.current;
    if (!canvas || !ctx || !frame) return;
    if (!visibleRef.current) {
      if (canvas.width !== 1) {
        canvas.width = 1;
        canvas.height = 1;
      }
      return;
    }
    const nCols = frame.cols;
    const nRows = frame.rows;
    if (maskRef.current) {
      if (nCols === colsRef.current && nRows === rowsRef.current) {
        maskRef.current = false;
      } else {
        const dpr = window.devicePixelRatio || 1;
        const rect = canvas.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;
        const bw = Math.round(rect.width * dpr);
        const bh = Math.round(rect.height * dpr);
        if (canvas.width !== bw || canvas.height !== bh) {
          canvas.width = bw;
          canvas.height = bh;
        }
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, bw, bh);
        return;
      }
    }
    const w = nCols * cellW;
    const h = nRows * CELL_H;
    const k = kRef.current;
    const moving = Math.abs(k - prevKRef.current) > 1e-4;
    prevKRef.current = k;
    if (moving) {
      clearTimeout(settleRef.current);
      settleRef.current = setTimeout(() => {
        dirtyRef.current = true;
      }, 50);
      return;
    }
    const dpr = window.devicePixelRatio || 1;
    const rect = canvas.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    const bw = Math.round(rect.width * dpr);
    const bh = Math.round(rect.height * dpr);
    if (canvas.width !== bw || canvas.height !== bh) {
      canvas.width = bw;
      canvas.height = bh;
    }
    const s = k * dpr;
    ctx.setTransform(s, 0, 0, s, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.textBaseline = 'top';
    const snap = (v: number) => Math.round(v * s) / s;
    const snapY = snap;

    const drawBlock = (cp: number, c: number, r: number) => {
      const x0 = snap(c * cellW);
      const x1 = snap((c + 1) * cellW);
      const y0 = snapY(r * CELL_H);
      const y1 = snapY((r + 1) * CELL_H);
      const xm = snap(c * cellW + cellW / 2);
      const ym = snapY(r * CELL_H + CELL_H / 2);
      const rect = (l: number, t: number, rt: number, b: number) => ctx.fillRect(l, t, rt - l, b - t);
      if (cp === 0x2588) return rect(x0, y0, x1, y1);
      if (cp === 0x2580) return rect(x0, y0, x1, ym);
      if (cp === 0x2590) return rect(xm, y0, x1, y1);
      if (cp === 0x2594) return rect(x0, y0, x1, snapY(r * CELL_H + CELL_H / 8));
      if (cp === 0x2595) return rect(snap(c * cellW + (cellW * 7) / 8), y0, x1, y1);
      if (cp >= 0x2581 && cp <= 0x2587) {
        return rect(x0, snapY(r * CELL_H + CELL_H * (1 - (cp - 0x2580) / 8)), x1, y1);
      }
      if (cp >= 0x2589 && cp <= 0x258f) {
        return rect(x0, y0, snap(c * cellW + (cellW * (0x2590 - cp)) / 8), y1);
      }
      if (cp >= 0x2591 && cp <= 0x2593) {
        ctx.save();
        ctx.globalAlpha = (cp - 0x2590) / 4;
        rect(x0, y0, x1, y1);
        ctx.restore();
        return;
      }
      if (cp >= 0x2596 && cp <= 0x259f) {
        const q = QUAD[cp - 0x2596];
        if (q & 8) rect(x0, y0, xm, ym);
        if (q & 4) rect(xm, y0, x1, ym);
        if (q & 2) rect(x0, ym, xm, y1);
        if (q & 1) rect(xm, ym, x1, y1);
      }
    };

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
          const cp = ch.codePointAt(0) ?? 0;
          if (cp >= 0x2580 && cp <= 0x259f) {
            ctx.fillStyle = fgOf(w0);
            drawBlock(cp, c, r);
          } else {
            ctx.font = `${w0 & (1 << 24) ? 'bold ' : ''}${FONT}px Hack, monospace`;
            ctx.fillStyle = fgOf(w0);
            const box = cp >= 0x2500 && cp <= 0x257f;
            ctx.fillText(ch, box ? c * cellW : snap(c * cellW), box ? r * CELL_H + yOff : snapY(r * CELL_H + yOff));
          }
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

    const hov = hoverRef.current;
    if (hov) {
      ctx.fillStyle = termTheme.fg;
      for (const seg of hov.segments) {
        if (seg.row < 0 || seg.row >= nRows) continue;
        ctx.fillRect(seg.c0 * cellW, snapY(seg.row * CELL_H + CELL_H - 1), (seg.c1 - seg.c0 + 1) * cellW, 1);
      }
    }

    if (activeRef.current && blinkRef.current && !frame.cursorHidden) {
      ctx.fillStyle = termTheme.cursor;
      ctx.fillRect(frame.cursorCol * cellW, frame.cursorRow * CELL_H, cellW, CELL_H);
    }
  }, []);

  React.useEffect(() => {
    const onTheme = () => {
      dirtyRef.current = true;
    };
    window.addEventListener(THEME_EVENT, onTheme);
    return () => window.removeEventListener(THEME_EVENT, onTheme);
  }, []);

  React.useEffect(() => {
    const onKeyUp = (e: KeyboardEvent) => {
      if ((e.key === 'Control' || e.key === 'Meta') && hoverRef.current) {
        hoverRef.current = null;
        dirtyRef.current = true;
        if (canvasRef.current) canvasRef.current.style.cursor = '';
      }
    };
    window.addEventListener('keyup', onKeyUp);
    return () => window.removeEventListener('keyup', onKeyUp);
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
            dirtyRef.current = true;
          },
          onExit: () => {},
          onClaude: (state) => {
            const { reset, ...rest } = state;
            if (reset) statusRef.current = undefined;
            claudeRef.current = reset ? rest : { ...claudeRef.current, ...rest };
            onClaudeDiffRef.current?.(claudeRef.current.linesAdded ?? 0, claudeRef.current.linesRemoved ?? 0);
            const prev = statusRef.current;
            const next = state.status;
            if (next && next !== prev) {
              statusRef.current = next;
              onClaudeStatusRef.current?.(next);
              if (!isWatching() && !agentEventsRef.current) {
                if (prev === 'busy' && next === 'idle') notifyClaude(tileId, 'finished');
                else if (next === 'waiting') notifyClaude(tileId, 'attention');
              }
            }
          },
          onCwd: (dir, branch) => onCwdRef.current(tileId, dir, branch),
          onClipboard: (text) => writeClipboard(text),
          onTitle: (title) => onOscTitleRef.current(tileId, title),
          onNotify: (title, body) => {
            if (Date.now() - lastAgentEventRef.current < 5000) return;
            if (!isWatching()) notifyClaude(tileId, 'generic', body, title || undefined);
          },
          onProgress: (state, pct) => onProgressRef.current?.(state, pct),
          onAgentEvent: (evt) => {
            agentEventsRef.current = true;
            lastAgentEventRef.current = Date.now();
            if (evt.event === 'prompt-submit') {
              clearNotify(tileId);
              return;
            }
            if (isWatching()) return;
            if (evt.event === 'stop') {
              lastNotifyRef.current = Date.now();
              notifyClaude(tileId, 'finished', evt.response || undefined);
            } else if (evt.event === 'permission') {
              const detail = [evt.toolName, evt.message].filter(Boolean).join(': ');
              lastNotifyRef.current = Date.now();
              notifyClaude(tileId, 'permission', detail || undefined);
            } else if (evt.event === 'notification') {
              if (Date.now() - lastNotifyRef.current < NOTIFY_IDLE_GRACE_MS) return;
              if (!document.hasFocus()) notifyClaude(tileId, 'idle', evt.message || undefined);
            }
          },
          onReady: (info) => {
            const w = wsRef.current;
            if (w) sendPtyFocus(w, activeRef.current && document.hasFocus());
            if (!pendingResumeRef.current) return;
            pendingResumeRef.current = false;
            if (!info.resumeId || info.reused) return;
            const id = info.resumeId;
            setTimeout(() => {
              const w = wsRef.current;
              if (w) sendPtyInput(w, ` claude --resume ${id}\r`);
            }, 900);
          }
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
      clearTimeout(settleRef.current);
      wsRef.current?.close();
      wsRef.current = null;
      frameRef.current = null;
    };
  }, [tileId, cwd, draw]);

  React.useEffect(() => {
    const prev = prevDimsRef.current;
    prevDimsRef.current = { cols, rows };
    if (Math.abs(cols - prev.cols) + Math.abs(rows - prev.rows) > 12) {
      maskRef.current = true;
      dirtyRef.current = true;
    }
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) sendPtyResize(ws, cols, rows);
  }, [cols, rows]);

  React.useEffect(() => {
    dirtyRef.current = true;
  }, [k, visible]);

  const lastRestartRef = React.useRef(restartKey);
  React.useEffect(() => {
    if (lastRestartRef.current === restartKey) return;
    lastRestartRef.current = restartKey;
    const ws = wsRef.current;
    if (!ws) return;
    pendingResumeRef.current = true;
    sendPtyKill(ws);
    ws.close();
  }, [restartKey]);

  React.useEffect(() => {
    blinkRef.current = true;
    dirtyRef.current = true;
    if (!active) return;
    const id = setInterval(() => {
      blinkRef.current = !blinkRef.current;
      dirtyRef.current = true;
    }, 530);
    return () => clearInterval(id);
  }, [active]);

  React.useEffect(() => {
    const update = () => {
      const ws = wsRef.current;
      if (ws) sendPtyFocus(ws, activeRef.current && document.hasFocus());
    };
    update();
    window.addEventListener('focus', update);
    window.addEventListener('blur', update);
    return () => {
      window.removeEventListener('focus', update);
      window.removeEventListener('blur', update);
    };
  }, [active]);

  React.useEffect(() => {
    if (!active) return;
    const onCopyKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      if (e.key.toLowerCase() !== 'c') return;
      const sel = selRef.current;
      const frame = frameRef.current;
      if (!sel || !frame) return;
      const domSel = window.getSelection();
      if (domSel && !domSel.isCollapsed) return;
      e.preventDefault();
      e.stopPropagation();
      const text = selectText(frame.lines, frame.cols, sel);
      if (text) writeClipboard(text);
      if (!e.shiftKey) {
        selRef.current = null;
        dirtyRef.current = true;
      }
    };
    window.addEventListener('keydown', onCopyKey, true);
    return () => window.removeEventListener('keydown', onCopyKey, true);
  }, [active]);

  const cellFromEvent = (e: React.MouseEvent): Cell | null => {
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

  const modBits = (e: React.PointerEvent): number =>
    (e.altKey ? 8 : 0) | (e.ctrlKey || e.metaKey ? 16 : 0);

  const onPointerDown = (e: React.PointerEvent) => {
    if (!activeRef.current) return;
    const cell = cellFromEvent(e);
    if (!cell) return;

    const frame = frameRef.current;
    const ws = wsRef.current;
    if (e.button === 0 && (e.ctrlKey || e.metaKey) && frame) {
      const span = urlSpanAt(frame.lines, frame.cols, cell.row, cell.col);
      if (span) {
        e.preventDefault();
        e.stopPropagation();
        openUrl(span.url);
        return;
      }
    }
    if (ws && frame && frame.mouseMode > 0 && !e.shiftKey && e.button !== 1) {
      e.stopPropagation();
      canvasRef.current?.setPointerCapture(e.pointerId);
      mouseFwdRef.current = true;
      mouseBtnRef.current = e.button;
      lastFwdRef.current = cell;
      sendPtyMouse(ws, 0, e.button, cell.col + 1, cell.row + 1, modBits(e));
      return;
    }
    if (e.button !== 0) return;
    e.stopPropagation();

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
    if (mouseFwdRef.current) {
      const ws = wsRef.current;
      const frame = frameRef.current;
      if (!ws || !frame || frame.mouseMode < 3) return;
      const cell = cellFromEvent(e);
      if (!cell || (cell.row === lastFwdRef.current.row && cell.col === lastFwdRef.current.col)) return;
      lastFwdRef.current = cell;
      sendPtyMouse(ws, 2, mouseBtnRef.current, cell.col + 1, cell.row + 1, modBits(e));
      return;
    }
    if (!selectingRef.current || !selRef.current) {
      const ws = wsRef.current;
      const hoverFrame = frameRef.current;
      if (ws && hoverFrame && hoverFrame.mouseMode === 4 && activeRef.current && !e.shiftKey) {
        const cell = cellFromEvent(e);
        if (cell && (cell.row !== lastFwdRef.current.row || cell.col !== lastFwdRef.current.col)) {
          lastFwdRef.current = cell;
          sendPtyMouse(ws, 2, 3, cell.col + 1, cell.row + 1, modBits(e));
        }
      }
      const canvas = canvasRef.current;
      if (canvas) {
        const frame = frameRef.current;
        const cell = (e.ctrlKey || e.metaKey) && frame ? cellFromEvent(e) : null;
        const span = cell && frame ? urlSpanAt(frame.lines, frame.cols, cell.row, cell.col) : null;
        canvas.style.cursor = span ? 'pointer' : '';
        const prev = hoverRef.current;
        const seg = span?.segments[0];
        const pseg = prev?.segments[0];
        if (span?.url !== prev?.url || seg?.row !== pseg?.row || seg?.c0 !== pseg?.c0) {
          hoverRef.current = span;
          dirtyRef.current = true;
        }
      }
      return;
    }
    const cell = cellFromEvent(e);
    if (!cell) return;
    selRef.current = { a: selRef.current.a, b: cell };
    dirtyRef.current = true;
  };

  const onPointerUp = (e: React.PointerEvent) => {
    if (mouseFwdRef.current) {
      mouseFwdRef.current = false;
      canvasRef.current?.releasePointerCapture(e.pointerId);
      const ws = wsRef.current;
      const cell = cellFromEvent(e) ?? lastFwdRef.current;
      if (ws) sendPtyMouse(ws, 1, mouseBtnRef.current, cell.col + 1, cell.row + 1, modBits(e));
      return;
    }
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

  const onPointerLeave = () => {
    if (!mouseFwdRef.current) lastFwdRef.current = { row: -1, col: -1 };
    if (!hoverRef.current) return;
    hoverRef.current = null;
    dirtyRef.current = true;
    if (canvasRef.current) canvasRef.current.style.cursor = '';
  };

  const onWheel = (e: React.WheelEvent) => {
    if (!activeRef.current) return;
    const ws = wsRef.current;
    if (!ws) return;
    e.stopPropagation();
    const px = e.deltaMode === 1 ? e.deltaY * WHEEL_LINE_PX : e.deltaY;
    wheelAccRef.current += px;
    const lines = Math.trunc(wheelAccRef.current / WHEEL_LINE_PX);
    if (lines === 0) return;
    wheelAccRef.current -= lines * WHEEL_LINE_PX;
    const cell = cellFromEvent(e);
    clearSelection();
    sendPtyScroll(ws, lines < 0 ? 1 : -1, Math.abs(lines), (cell?.col ?? 0) + 1, (cell?.row ?? 0) + 1);
  };

  const sendData = React.useCallback((data: string) => {
    const ws = wsRef.current;
    if (ws) sendPtyInput(ws, data);
  }, []);

  const getLines = React.useCallback(() => frameRef.current?.lines ?? [], []);

  const getStructured = React.useCallback(() => claudeRef.current, []);

  const focusTerminal = React.useCallback(() => canvasRef.current?.focus({ preventScroll: true }), []);

  return (
    <>
      <canvas
        ref={canvasRef}
        style={{ top: 'auto', bottom: devicePx(6 * k), height: devicePx(rows * CELL_H * k) }}
        tabIndex={-1}
        onWheel={onWheel}
        onKeyDown={onKeyDown}
        onPointerUp={onPointerUp}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerLeave={onPointerLeave}
        onPointerCancel={onPointerUp}
        onContextMenu={onContextMenu}
        className={styles.wasm}
      />
      <div className={styles.agentScale} style={{ width: `calc(100% / ${k})`, height: `calc(100% / ${k})`, transform: `scale(${k})` }}>
        <AgentBar
          tileId={tileId}
          active={active}
          send={sendData}
          getLines={getLines}
          getStructured={getStructured}
          focusTerminal={focusTerminal}
          onClaudeActive={onClaudeActive}
        />
      </div>
    </>
  );
};

export default GridTerminal;
