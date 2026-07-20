import React from 'react';

import AgentBar from '~/components/Terminal/AgentBar';
import type { AgentType } from '~/components/Terminal/AgentBar/parse';
import ResumePanel from '~/components/Terminal/ResumePanel';
import { termTheme, THEME_EVENT } from '~/usecase/util/theme';
import { getSetting } from '~/adapter/settings/settings.client';
import { scheduleConnect } from '~/usecase/util/connectScheduler';
import { TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
import { keyToBytes } from '~/usecase/util/terminalKeys';
import { hasAgentUi } from '~/components/Terminal/AgentBar/parse';
import { notifyClaude, clearNotify } from '~/components/commons/Notifications/bridge';
import { openUrl } from '~/adapter/shell/shell.client';
import { urlSpanAt, orderSel, selectText, lineSelection, wordSelection } from '~/usecase/util/terminalSelection';
import { readClipboard, writeClipboard, hasClipboardImage } from '~/adapter/clipboard/clipboard.client';
import { sendPtyKill, sendPtyMouse, sendPtyFocus, sendPtyInput, sendPtyScroll, sendPtyResize, sendPtyVisible, openPtyConnection, sendPtyDismissAgent } from '~/adapter/pty/pty.client';

import type { GridFrame, ClaudeState } from '~/domain/interfaces/pty.interface';
import type { Cell, Selection, UrlSpan } from '~/usecase/util/terminalSelection';

import styles from './styles.module.scss';

interface GridTerminalProps {
  tileId: string;
  sessionId?: string;
  readOnly?: boolean;
  cwd?: string;
  cols: number;
  rows: number;
  active: boolean;
  visible: boolean;
  elevated: boolean;
  restartKey: number;
  onCwd: (id: string, cwd: string, branch?: string) => void;
  onOscTitle: (id: string, title: string) => void;
  onAgentActive?: (type: AgentType | null) => void;
  onClaudeStatus?: (status: string) => void;
  onClaudeDiff?: (added: number, removed: number) => void;
  onProgress?: (state: number, pct: number) => void;
  onContextMenu?: (e: React.MouseEvent) => void;
}

const CELL_H = 15;
const WHEEL_LINE_PX = 100 / 3;
const PAINT_MS = 60;
const CLICK_MS = 400;
const NOTIFY_IDLE_GRACE_MS = 10000;
const DEFAULT_FG = 0xc7d0e0;
const NO_LINES: string[] = [];
const isLinux = /linux/i.test(navigator.userAgent);

const hexCache = new Map<number, string>();
const hex = (v: number): string => {
  const key = v & 0xffffff;
  let out = hexCache.get(key);
  if (!out) {
    if (hexCache.size > 4096) hexCache.clear();
    out = '#' + key.toString(16).padStart(6, '0');
    hexCache.set(key, out);
  }
  return out;
};

const ASTRAL = /[\uD800-\uDBFF]/;
const fgOf = (w0: number): string => {
  const v = w0 & 0xffffff;
  if (v === DEFAULT_FG) return termTheme.fg;
  return termTheme.ansi?.get(v) ?? hex(v);
};

const esc = (s: string): string => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');

const isSym = (cp: number): boolean =>
  (cp >= 0x2300 && cp <= 0x23ff) || (cp >= 0x25a0 && cp <= 0x27bf) || cp === 0x2217;

let symCtx: CanvasRenderingContext2D | null = null;
let symRefInk = 0;
const symScaleCache = new Map<number, number>();

const inkOf = (ch: string): number => {
  if (!symCtx) symCtx = document.createElement('canvas').getContext('2d');
  if (!symCtx) return 0;
  symCtx.font = "12px 'Segoe UI Symbol'";
  const m = symCtx.measureText(ch);
  const h = (m.actualBoundingBoxAscent ?? 0) + (m.actualBoundingBoxDescent ?? 0);
  const w = (m.actualBoundingBoxLeft ?? 0) + (m.actualBoundingBoxRight ?? 0);
  return Math.max(h, w);
};

const symScale = (cp: number, ch: string): number => {
  let s = symScaleCache.get(cp);
  if (s === undefined) {
    if (!symRefInk) symRefInk = inkOf('✻') || 9;
    const ink = inkOf(ch);
    s = ink > 0 ? Math.min(1.5, Math.max(0.7, symRefInk / ink)) : 1;
    symScaleCache.set(cp, s);
  }
  return s;
};

const isWide = (cp: number): boolean =>
  cp >= 0x1100 &&
  (cp <= 0x115f ||
    (cp >= 0x2e80 && cp <= 0xa4cf) ||
    (cp >= 0xac00 && cp <= 0xd7a3) ||
    (cp >= 0xf900 && cp <= 0xfaff) ||
    (cp >= 0xfe30 && cp <= 0xfe4f) ||
    (cp >= 0xff00 && cp <= 0xff60) ||
    (cp >= 0xffe0 && cp <= 0xffe6) ||
    cp >= 0x1f000);

const QUAD = [0b0010, 0b0001, 0b1000, 0b1011, 0b1001, 0b1110, 0b1101, 0b0100, 0b0110, 0b0111];

const quadLayer = (a: number, b: number, q: number): string => {
  const l = q & a ? 'currentcolor' : 'transparent';
  const r = q & b ? 'currentcolor' : 'transparent';
  return `linear-gradient(to right,${l} 50%,${r} 50%)`;
};

const grad = (img: string): string => `background-image:${img}`;

const blockCss = (cp: number): string => {
  if (cp === 0x2588) return grad('linear-gradient(currentcolor,currentcolor)');
  if (cp === 0x2580) return grad('linear-gradient(currentcolor 50%,transparent 50%)');
  if (cp >= 0x2581 && cp <= 0x2587) {
    const pct = ((0x2588 - cp) / 8) * 100;
    return grad(`linear-gradient(transparent ${pct}%,currentcolor ${pct}%)`);
  }
  if (cp >= 0x2589 && cp <= 0x258f) {
    const pct = ((0x2590 - cp) / 8) * 100;
    return grad(`linear-gradient(to right,currentcolor ${pct}%,transparent ${pct}%)`);
  }
  if (cp === 0x2590) return grad('linear-gradient(to right,transparent 50%,currentcolor 50%)');
  if (cp >= 0x2591 && cp <= 0x2593) {
    return `${grad('linear-gradient(currentcolor,currentcolor)')};opacity:${(cp - 0x2590) * 0.25}`;
  }
  if (cp === 0x2594) return grad('linear-gradient(currentcolor 12.5%,transparent 12.5%)');
  if (cp === 0x2595) return grad('linear-gradient(to right,transparent 87.5%,currentcolor 87.5%)');
  const q = QUAD[cp - 0x2596];
  return `${grad(`${quadLayer(8, 4, q)},${quadLayer(2, 1, q)}`)};background-size:100% 50%;background-position:top,bottom;background-repeat:no-repeat`;
};

const rowHtml = (line: string, attrs: Uint32Array, r: number, nCols: number): string => {
  const cells = ASTRAL.test(line) ? Array.from(line) : line;
  let html = '';
  let runStyle = '';
  let runText = '';
  const flush = () => {
    if (!runText) return;
    html += runStyle ? `<span style="${runStyle}">${esc(runText)}</span>` : esc(runText);
    runText = '';
  };
  for (let c = 0; c < nCols; c++) {
    const i = (r * nCols + c) * 2;
    const w0 = attrs[i] ?? 0;
    const w1 = attrs[i + 1] ?? 0;
    const bold = (w0 & (1 << 24)) !== 0;
    const hasBg = (w1 & 0x80000000) !== 0;
    const ch = cells[c] ?? ' ';
    const cp = ch.codePointAt(0) ?? 0;
    const block = cp >= 0x2580 && cp <= 0x259f;
    let style: string;
    let text: string;
    if (!block && isSym(cp)) {
      flush();
      runStyle = '';
      const isDefault = (w0 & 0xffffff) === DEFAULT_FG && !bold && !hasBg;
      const symStyle = `width:7.23px;text-align:center;font-family:'Segoe UI Symbol',monospace;font-size:${(12 * symScale(cp, ch)).toFixed(2)}px${
        isDefault ? '' : `;color:${fgOf(w0)}${bold ? ';font-weight:700' : ''}${hasBg ? `;background:${hex(w1)}` : ''}`
      }`;
      html += `<span style="${symStyle}">${esc(ch)}</span>`;
      continue;
    }
    if (!block && isWide(cp) && (cells[c + 1] ?? ' ') === ' ') {
      flush();
      runStyle = '';
      const isDefault = (w0 & 0xffffff) === DEFAULT_FG && !bold && !hasBg;
      const wideStyle = isDefault
        ? 'width:2ch'
        : `width:2ch;color:${fgOf(w0)}${bold ? ';font-weight:700' : ''}${hasBg ? `;background:${hex(w1)}` : ''}`;
      html += `<span style="${wideStyle}">${esc(ch)}</span>`;
      c++;
      continue;
    }
    if (block) {
      style = `color:${fgOf(w0)};${blockCss(cp)}${hasBg ? `;background-color:${hex(w1)}` : ''}`;
      text = ' ';
    } else {
      const isDefault = (w0 & 0xffffff) === DEFAULT_FG && !bold && !hasBg;
      style = isDefault
        ? ''
        : `color:${fgOf(w0)}${bold ? ';font-weight:700' : ''}${hasBg ? `;background:${hex(w1)}` : ''}`;
      text = ch;
    }
    if (style !== runStyle) {
      flush();
      runStyle = style;
    }
    runText += text;
  }
  flush();
  return html;
};

const GridTerminal = ({ tileId, sessionId, readOnly, cwd, cols, rows, active, visible, elevated, restartKey, onCwd, onOscTitle, onAgentActive, onClaudeStatus, onClaudeDiff, onProgress, onContextMenu }: GridTerminalProps) => {
  const [resumeId, setResumeId] = React.useState<string | null>(null);
  const termRef = React.useRef<HTMLDivElement>(null);
  const rowsRefEl = React.useRef<HTMLDivElement>(null);
  const overlayRef = React.useRef<HTMLDivElement>(null);
  const cursorRef = React.useRef<HTMLDivElement>(null);
  const rowCacheRef = React.useRef<string[]>([]);
  const textareaRef = React.useRef<HTMLTextAreaElement>(null);
  const wsRef = React.useRef<WebSocket | null>(null);
  const frameRef = React.useRef<GridFrame | null>(null);
  const claudeRef = React.useRef<ClaudeState | null>(null);
  const statusRef = React.useRef<string | undefined>(undefined);
  const dirtyRef = React.useRef(true);
  const selRef = React.useRef<Selection | null>(null);
  const hoverRef = React.useRef<UrlSpan | null>(null);
  const selectingRef = React.useRef(false);
  const clickRef = React.useRef({ t: 0, row: -1, col: -1, count: 0 });
  const mouseFwdRef = React.useRef(false);
  const mouseBtnRef = React.useRef(0);
  const lastFwdRef = React.useRef({ row: -1, col: -1 });
  const wheelAccRef = React.useRef(0);
  const pendingResumeRef = React.useRef(true);
  const resumeCandidateRef = React.useRef<string | null>(null);
  const activeRef = React.useRef(active);
  const visibleRef = React.useRef(visible);
  const elevatedRef = React.useRef(elevated);
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
  elevatedRef.current = elevated;
  colsRef.current = cols;
  rowsRef.current = rows;
  onCwdRef.current = onCwd;
  onOscTitleRef.current = onOscTitle;
  onClaudeStatusRef.current = onClaudeStatus;
  onClaudeDiffRef.current = onClaudeDiff;
  onProgressRef.current = onProgress;

  const focusTerminal = React.useCallback(() => {
    const el = isLinux ? textareaRef.current : termRef.current;
    el?.focus({ preventScroll: true });
  }, []);

  const isWatching = React.useCallback((): boolean => {
    const rect = termRef.current?.getBoundingClientRect();
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
    const rowsEl = rowsRefEl.current;
    const overlay = overlayRef.current;
    const cursor = cursorRef.current;
    const frame = frameRef.current;
    if (!rowsEl || !overlay || !cursor || !frame) return;
    if (!visibleRef.current) return;
    const nCols = frame.cols;
    const nRows = frame.rows;
    const cache = rowCacheRef.current;

    while (rowsEl.children.length > nRows) rowsEl.lastElementChild?.remove();
    while (rowsEl.children.length < nRows) {
      const d = document.createElement('div');
      d.className = styles.row;
      rowsEl.appendChild(d);
    }
    cache.length = nRows;

    const lines = frame.lines;
    const attrs = frame.attrs;
    for (let r = 0; r < nRows; r++) {
      const html = rowHtml(lines[r] ?? '', attrs, r, nCols);
      if (cache[r] === html) continue;
      cache[r] = html;
      (rowsEl.children[r] as HTMLElement).innerHTML = html;
    }

    let ov = '';
    const sel = selRef.current;
    if (sel) {
      const { s, e } = orderSel(sel);
      for (let r = s.row; r <= e.row; r++) {
        if (r < 0 || r >= nRows) continue;
        const c0 = r === s.row ? s.col : 0;
        const c1 = r === e.row ? e.col : nCols - 1;
        ov += `<div class="${styles.selRect}" style="left:${c0}ch;top:${r * CELL_H}px;width:${c1 - c0 + 1}ch"></div>`;
      }
    }
    const hov = hoverRef.current;
    if (hov) {
      for (const seg of hov.segments) {
        if (seg.row < 0 || seg.row >= nRows) continue;
        ov += `<div class="${styles.urlLine}" style="left:${seg.c0}ch;top:${seg.row * CELL_H + CELL_H - 1}px;width:${seg.c1 - seg.c0 + 1}ch"></div>`;
      }
    }
    if (overlay.innerHTML !== ov) overlay.innerHTML = ov;

    const showCursor = activeRef.current && !frame.cursorHidden;
    cursor.style.display = showCursor ? '' : 'none';
    if (showCursor) {
      cursor.style.left = `${frame.cursorCol}ch`;
      cursor.style.top = `${frame.cursorRow * CELL_H}px`;
    }
  }, []);

  React.useEffect(() => {
    const applyTheme = () => {
      const term = termRef.current;
      const cursor = cursorRef.current;
      if (term) term.style.color = termTheme.fg;
      if (cursor) cursor.style.background = termTheme.cursor;
      rowCacheRef.current = [];
      dirtyRef.current = true;
    };
    applyTheme();
    window.addEventListener(THEME_EVENT, applyTheme);
    return () => window.removeEventListener(THEME_EVENT, applyTheme);
  }, []);

  React.useEffect(() => {
    const onKeyUp = (e: KeyboardEvent) => {
      if ((e.key === 'Control' || e.key === 'Meta') && hoverRef.current) {
        hoverRef.current = null;
        dirtyRef.current = true;
        if (termRef.current) termRef.current.style.cursor = '';
      }
    };
    window.addEventListener('keyup', onKeyUp);
    return () => window.removeEventListener('keyup', onKeyUp);
  }, []);

  React.useEffect(() => {
    let disposed = false;
    let exited = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    const target = getSetting(TERMINAL_TARGET_KEY, 'auto');
    const paint = setInterval(() => {
      if (!dirtyRef.current) return;
      dirtyRef.current = false;
      draw();
    }, PAINT_MS);

    const connect = () => {
      if (disposed) return;
      const ws = openPtyConnection(
        {
          tileId: sessionId ?? tileId,
          cwd: readOnly ? undefined : cwd,
          cols: colsRef.current,
          rows: rowsRef.current,
          target: readOnly ? undefined : target,
          elevated: elevatedRef.current,
          attach: readOnly
        },
        {
          acceptGrid: () => visibleRef.current && !document.hidden,
          onGrid: (frame) => {
            frameRef.current = frame;
            dirtyRef.current = true;
            if (readOnly) return;
            const candidate = resumeCandidateRef.current;
            if (!candidate) return;
            resumeCandidateRef.current = null;
            if (!hasAgentUi(frame.lines.join('\n'))) setResumeId(candidate);
          },
          onExit: () => {
            exited = true;
          },
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
            if (w) {
              sendPtyFocus(w, activeRef.current && document.hasFocus());
              sendPtyVisible(w, visibleRef.current && !document.hidden);
            }
            if (!pendingResumeRef.current) return;
            pendingResumeRef.current = false;
            if (!info.resumeId) return;
            resumeCandidateRef.current = info.resumeId;
          }
        }
      );
      wsRef.current = ws;
      ws.onclose = () => {
        if (disposed || (readOnly && exited)) return;
        retry = setTimeout(() => scheduleConnect(connect, visibleRef.current ? 2 : 1), 2000);
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
  }, [tileId, sessionId, readOnly, cwd, draw]);

  React.useEffect(() => {
    const ws = wsRef.current;
    if (ws && ws.readyState === WebSocket.OPEN) sendPtyResize(ws, cols, rows);
  }, [cols, rows]);

  React.useEffect(() => {
    dirtyRef.current = true;
  }, [visible]);

  React.useEffect(() => {
    const ws = wsRef.current;
    if (ws) sendPtyVisible(ws, visible && !document.hidden);
  }, [visible]);

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
    dirtyRef.current = true;
    if (active && readOnly) focusTerminal();
  }, [active, readOnly, focusTerminal]);

  React.useEffect(() => {
    const update = () => {
      const ws = wsRef.current;
      if (ws) {
        sendPtyFocus(ws, activeRef.current && document.hasFocus());
        sendPtyVisible(ws, visibleRef.current && !document.hidden);
      }
    };
    update();
    window.addEventListener('focus', update);
    window.addEventListener('blur', update);
    document.addEventListener('visibilitychange', update);
    return () => {
      window.removeEventListener('focus', update);
      window.removeEventListener('blur', update);
      document.removeEventListener('visibilitychange', update);
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
    const el = termRef.current;
    const frame = frameRef.current;
    if (!el || !frame) return null;
    const rect = el.getBoundingClientRect();
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
    if (ws && frame && frame.mouseMode > 0 && !readOnly && !e.shiftKey && e.button !== 1) {
      e.stopPropagation();
      termRef.current?.setPointerCapture(e.pointerId);
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

    termRef.current?.setPointerCapture(e.pointerId);
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
      if (ws && hoverFrame && hoverFrame.mouseMode === 4 && !readOnly && activeRef.current && !e.shiftKey) {
        const cell = cellFromEvent(e);
        if (cell && (cell.row !== lastFwdRef.current.row || cell.col !== lastFwdRef.current.col)) {
          lastFwdRef.current = cell;
          sendPtyMouse(ws, 2, 3, cell.col + 1, cell.row + 1, modBits(e));
        }
      }
      const el = termRef.current;
      if (el) {
        const frame = frameRef.current;
        const cell = (e.ctrlKey || e.metaKey) && frame ? cellFromEvent(e) : null;
        const span = cell && frame ? urlSpanAt(frame.lines, frame.cols, cell.row, cell.col) : null;
        el.style.cursor = span ? 'pointer' : '';
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
      termRef.current?.releasePointerCapture(e.pointerId);
      const ws = wsRef.current;
      const cell = cellFromEvent(e) ?? lastFwdRef.current;
      if (ws) sendPtyMouse(ws, 1, mouseBtnRef.current, cell.col + 1, cell.row + 1, modBits(e));
      return;
    }
    if (!selectingRef.current) return;
    selectingRef.current = false;
    termRef.current?.releasePointerCapture(e.pointerId);
    const s = selRef.current;
    if (s && s.a.row === s.b.row && s.a.col === s.b.col) selRef.current = null;
    dirtyRef.current = true;
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
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
      if (readOnly) return;
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

    if (readOnly) return;

    if (isLinux && ((e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) || e.key === 'Dead')) {
      return;
    }

    const bytes = keyToBytes(e);
    if (bytes === null) return;
    e.preventDefault();
    clearSelection();
    sendPtyInput(ws, bytes);
  };

  const onInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const ws = wsRef.current;
    if (!ws) return;

    const nativeEvent = e.nativeEvent as InputEvent;
    if (nativeEvent.isComposing) return;

    const target = e.target as HTMLTextAreaElement;
    const val = target.value;
    if (val) {
      clearSelection();
      sendPtyInput(ws, val);
      target.value = '';
    }
  };

  const onPointerLeave = () => {
    if (!mouseFwdRef.current) lastFwdRef.current = { row: -1, col: -1 };
    if (!hoverRef.current) return;
    hoverRef.current = null;
    dirtyRef.current = true;
    if (termRef.current) termRef.current.style.cursor = '';
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

  const getLines = React.useCallback(() => frameRef.current?.lines ?? NO_LINES, []);

  const getStructured = React.useCallback(() => claudeRef.current, []);

  const onTermFocus = () => {
    if (isLinux) textareaRef.current?.focus({ preventScroll: true });
  };

  const onTermKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (isLinux) return;
    onKeyDown(e as any);
  };

  const closeResume = () => {
    setResumeId(null);
    focusTerminal();
  };

  const dismissResume = () => {
    const ws = wsRef.current;
    if (ws) sendPtyDismissAgent(ws);
    closeResume();
  };

  const startResume = () => {
    const ws = wsRef.current;
    if (ws) sendPtyInput(ws, ` claude --resume ${resumeId}\r`);
    closeResume();
  };

  return (
    <>
      {isLinux && (
        <textarea
          ref={textareaRef}
          style={{
            position: 'absolute',
            left: '-9999px',
            top: '-9999px',
            width: '1px',
            height: '1px',
            opacity: 0,
            pointerEvents: 'none'
          }}
          tabIndex={-1}
          onKeyDown={onKeyDown}
          onInput={onInput}
        />
      )}
      <div
        ref={termRef}
        className={styles.term}
        tabIndex={-1}
        onWheel={onWheel}
        onKeyDown={onTermKeyDown}
        onFocus={onTermFocus}
        onPointerUp={onPointerUp}
        onPointerDown={(e) => {
          focusTerminal();
          onPointerDown(e);
        }}
        onPointerMove={onPointerMove}
        onPointerLeave={onPointerLeave}
        onPointerCancel={onPointerUp}
        onContextMenu={onContextMenu}
      >
        <div ref={rowsRefEl} className={styles.rows} />
        <div ref={overlayRef} className={styles.overlay} />
        <div ref={cursorRef} className={styles.cursor} />
      </div>
      {!readOnly && (
        <div className={styles.agentOverlay}>
          {resumeId && (
            <ResumePanel sessionId={resumeId} cwd={cwd} active={active} onResume={startResume} onSkip={dismissResume} />
          )}
          <AgentBar
            tileId={tileId}
            active={active}
            send={sendData}
            getLines={getLines}
            getStructured={getStructured}
            focusTerminal={focusTerminal}
            onAgentActive={onAgentActive}
          />
        </div>
      )}
    </>
  );
};

export default GridTerminal;
