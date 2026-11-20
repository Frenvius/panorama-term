import type { TileOpenRequest } from '~/domain/interfaces/mobile.interface';

import { SIDECAR_WS } from '~/adapter/pty/sidecar.client';

const RECONNECT_MS = 2000;

type TileOpenHandler = (req: TileOpenRequest) => void;
type OrchChangedHandler = (runId: string) => void;
type ConnectHandler = () => void;

interface OrchSub {
  onChanged: OrchChangedHandler;
  onConnect: ConnectHandler;
}

const tileOpenSubs = new Set<TileOpenHandler>();
const orchSubs = new Set<OrchSub>();

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

const totalSubs = (): number => tileOpenSubs.size + orchSubs.size;

const dispatch = (data: string): void => {
  let msg: { t: string; tileId?: string; cwd?: string; group?: { id: string; title: string }; runId?: string };
  try {
    msg = JSON.parse(data) as typeof msg;
  } catch {
    return;
  }
  if (msg.t === 'tile.open') {
    for (const fn of tileOpenSubs) fn(msg as TileOpenRequest);
  } else if (msg.t === 'orch.changed') {
    for (const sub of orchSubs) sub.onChanged(msg.runId ?? '');
  }
};

const connect = (): void => {
  if (totalSubs() === 0) return;
  ws = new WebSocket(`${SIDECAR_WS}/ui`);
  ws.onopen = () => {
    for (const sub of orchSubs) sub.onConnect();
  };
  ws.onmessage = (e) => {
    if (typeof e.data === 'string') dispatch(e.data);
  };
  ws.onclose = () => {
    ws = null;
    if (totalSubs() > 0) reconnectTimer = setTimeout(connect, RECONNECT_MS);
  };
};

const ensureConnected = (): void => {
  if (!ws && totalSubs() > 0) connect();
};

const maybeClose = (): void => {
  if (totalSubs() === 0) {
    clearTimeout(reconnectTimer);
    ws?.close();
    ws = null;
  }
};

export const watchTileRequests = (onOpen: TileOpenHandler): (() => void) => {
  tileOpenSubs.add(onOpen);
  ensureConnected();
  return () => {
    tileOpenSubs.delete(onOpen);
    maybeClose();
  };
};

export const watchOrchEvents = (onChanged: OrchChangedHandler, onConnect: ConnectHandler): (() => void) => {
  const sub: OrchSub = { onChanged, onConnect };
  orchSubs.add(sub);
  ensureConnected();
  return () => {
    orchSubs.delete(sub);
    maybeClose();
  };
};
