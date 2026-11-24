import type { TileOpenRequest } from '~/domain/interfaces/mobile.interface';
import type { OrchNotification } from '~/usecase/util/orchNotify';

import { SIDECAR_WS } from '~/adapter/pty/sidecar.client';

const RECONNECT_MS = 2000;

type TileOpenHandler = (req: TileOpenRequest) => void;
type TileWakeHandler = (tileId: string) => void;
type OrchChangedHandler = (runId: string) => void;
type ConnectHandler = () => void;

interface OrchSub {
  onChanged: OrchChangedHandler;
  onConnect: ConnectHandler;
}

const tileOpenSubs = new Set<TileOpenHandler>();
const tileWakeSubs = new Set<TileWakeHandler>();
const orchSubs = new Set<OrchSub>();
const orchNotifySubs = new Set<(event: OrchNotification) => void>();

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

const totalSubs = (): number => tileOpenSubs.size + tileWakeSubs.size + orchSubs.size + orchNotifySubs.size;

const dispatch = (data: string): void => {
  let msg: { t: string; tileId?: string; cwd?: string; group?: { id: string; title: string }; runId?: string };
  try {
    msg = JSON.parse(data) as typeof msg;
  } catch {
    return;
  }
  if (msg.t === 'tile.open') {
    for (const fn of tileOpenSubs) fn(msg as TileOpenRequest);
  } else if (msg.t === 'tile.wake') {
    for (const fn of tileWakeSubs) fn(msg.tileId ?? '');
  } else if (msg.t === 'orch.changed') {
    for (const sub of orchSubs) sub.onChanged(msg.runId ?? '');
  } else if (msg.t === 'orch.notify') {
    for (const fn of orchNotifySubs) fn(msg as unknown as OrchNotification);
  }
};

export const watchOrchNotifications = (onNotify: (event: OrchNotification) => void): (() => void) => {
  orchNotifySubs.add(onNotify);
  ensureConnected();
  return () => {
    orchNotifySubs.delete(onNotify);
    maybeClose();
  };
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

export const watchWakeRequests = (onWake: TileWakeHandler): (() => void) => {
  tileWakeSubs.add(onWake);
  ensureConnected();
  return () => {
    tileWakeSubs.delete(onWake);
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
