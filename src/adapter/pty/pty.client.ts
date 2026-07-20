import type { GridFrame, AgentEvent, ClaudeState, PtyServerMessage } from '~/domain/interfaces/pty.interface';

const SIDECAR_WS = 'ws://127.0.0.1:9777';

const decoder = new TextDecoder();

export interface PtyReadyInfo {
  reused: boolean;
  cols: number;
  rows: number;
  resumeId: string | null;
}

export interface PtyConnectionParams {
  cols: number;
  rows: number;
  tileId: string;
  cwd?: string;
  target?: string;
  elevated?: boolean;
  attach?: boolean;
}

export interface PtyHandlers {
  acceptGrid: () => boolean;
  onExit: () => void;
  onReady: (info: PtyReadyInfo) => void;
  onGrid: (frame: GridFrame) => void;
  onCwd: (cwd: string, branch?: string) => void;
  onClaude: (state: ClaudeState) => void;
  onClipboard: (text: string) => void;
  onTitle: (title: string) => void;
  onNotify: (title: string, body: string) => void;
  onAgentEvent: (event: AgentEvent) => void;
  onProgress: (state: number, pct: number) => void;
}

const parseGridFrame = (buf: ArrayBuffer): GridFrame | null => {
  const dv = new DataView(buf);
  if (dv.getUint8(0) !== 1) return null;
  const rows = dv.getUint16(1, true);
  const cols = dv.getUint16(3, true);
  const cursor = dv.getUint32(5, true);
  const cursorHidden = dv.getUint8(9) === 1;
  const mouseMode = dv.getUint8(10);
  const offset = dv.getUint16(11, true);
  const textLen = dv.getUint32(13, true);
  const text = decoder.decode(new Uint8Array(buf, 17, textLen));
  const attrs = new Uint32Array(buf.slice(17 + textLen));
  return {
    rows,
    cols,
    cursorRow: cursor >>> 16,
    cursorCol: cursor & 0xffff,
    cursorHidden,
    mouseMode,
    offset,
    lines: text.split('\n'),
    attrs
  };
};

export const openPtyConnection = (params: PtyConnectionParams, handlers: PtyHandlers): WebSocket => {
  const { tileId, cols, rows, cwd, target, elevated, attach } = params;
  let query = `tileId=${encodeURIComponent(tileId)}&cols=${cols}&rows=${rows}`;
  if (cwd) query += `&cwd=${encodeURIComponent(cwd)}`;
  if (target) query += `&target=${encodeURIComponent(target)}`;
  if (elevated) query += '&elevated=1';
  if (attach) query += '&attach=1';
  const ws = new WebSocket(`${SIDECAR_WS}/pty?${query}`);
  ws.binaryType = 'arraybuffer';
  ws.onmessage = (e) => {
    if (typeof e.data === 'string') {
      const msg = JSON.parse(e.data) as PtyServerMessage;
      if (msg.t === 'ready') handlers.onReady({ reused: msg.reused, cols: msg.cols, rows: msg.rows, resumeId: msg.resumeId });
      else if (msg.t === 'exit') handlers.onExit();
      else if (msg.t === 'error') handlers.onNotify('Terminal', msg.msg);
      else if (msg.t === 'cwd') handlers.onCwd(msg.cwd, msg.branch ?? undefined);
      else if (msg.t === 'claude') handlers.onClaude(msg);
      else if (msg.t === 'clipboard') handlers.onClipboard(msg.text);
      else if (msg.t === 'title') handlers.onTitle(msg.title);
      else if (msg.t === 'notify') handlers.onNotify(msg.title, msg.body);
      else if (msg.t === 'agentEvent') handlers.onAgentEvent(msg);
      else if (msg.t === 'progress') handlers.onProgress(msg.state, msg.pct);
      return;
    }
    if (!handlers.acceptGrid()) return;
    const frame = parseGridFrame(e.data as ArrayBuffer);
    if (frame) handlers.onGrid(frame);
  };
  return ws;
};

export const sendPtyInput = (ws: WebSocket, data: string): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'in', d: data }));
};

export const sendPtyResize = (ws: WebSocket, cols: number, rows: number): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'resize', cols, rows }));
};

export const sendPtyScroll = (ws: WebSocket, dir: number, lines: number, col: number, row: number): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'scroll', dir, lines, col, row }));
};

export const sendPtyMouse = (
  ws: WebSocket,
  kind: number,
  button: number,
  col: number,
  row: number,
  mods: number
): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'mouse', kind, button, col, row, mods }));
};

export const sendPtyFocus = (ws: WebSocket, focused: boolean): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'focus', focused }));
};

export const sendPtyVisible = (ws: WebSocket, visible: boolean): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'visible', visible }));
};

export const sendPtyDismissAgent = (ws: WebSocket): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'dismissAgent' }));
};

export const sendPtyKill = (ws: WebSocket): void => {
  if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ t: 'kill' }));
};
