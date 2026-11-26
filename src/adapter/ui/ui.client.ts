import type { TileOpenRequest } from '~/domain/interfaces/mobile.interface';
import type { OrchNotification } from '~/usecase/util/orchNotify';

import { SIDECAR_WS } from '~/adapter/pty/sidecar.client';

const RECONNECT_MS = 2000;

type TileOpenHandler = (req: TileOpenRequest) => void;
type TileWakeHandler = (tileId: string) => void;
type OrchChangedHandler = (runId: string) => void;
type ConnectHandler = () => void;
type AppUpdateHandler = () => void;
type AgentChangedHandler = () => void;
type AgentMailHandler = (parties: string[]) => void;

interface OrchSub {
  onChanged: OrchChangedHandler;
  onConnect: ConnectHandler;
}

interface AgentSub {
  onChanged: AgentChangedHandler;
  onConnect: ConnectHandler;
}

interface AgentMailSub {
  onMail: AgentMailHandler;
  onConnect: ConnectHandler;
}

const tileOpenSubs = new Set<TileOpenHandler>();
const tileWakeSubs = new Set<TileWakeHandler>();
const orchSubs = new Set<OrchSub>();
const orchNotifySubs = new Set<(event: OrchNotification) => void>();
const appUpdateSubs = new Set<AppUpdateHandler>();
const agentSubs = new Set<AgentSub>();
const agentMailSubs = new Set<AgentMailSub>();

let ws: WebSocket | null = null;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

const totalSubs = (): number =>
  tileOpenSubs.size + tileWakeSubs.size + orchSubs.size + orchNotifySubs.size + appUpdateSubs.size + agentSubs.size + agentMailSubs.size;

const dispatch = (data: string): void => {
  let msg: { t: string; tileId?: string; cwd?: string; group?: { id: string; title: string }; runId?: string; parties?: string[] };
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
  } else if (msg.t === 'app.update') {
    for (const fn of appUpdateSubs) fn();
  } else if (msg.t === 'agents.changed' || msg.t === 'agents.waits') {
    for (const sub of agentSubs) sub.onChanged();
  } else if (msg.t === 'agents.mail') {
    const parties = msg.parties ?? [];
    for (const sub of agentMailSubs) sub.onMail(parties);
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
    for (const sub of agentSubs) sub.onConnect();
    for (const sub of agentMailSubs) sub.onConnect();
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

export const watchAppUpdateRequests = (onUpdate: AppUpdateHandler): (() => void) => {
  appUpdateSubs.add(onUpdate);
  ensureConnected();
  return () => {
    appUpdateSubs.delete(onUpdate);
    maybeClose();
  };
};

export const watchAgentEvents = (onChanged: AgentChangedHandler, onConnect: ConnectHandler): (() => void) => {
  const sub: AgentSub = { onChanged, onConnect };
  agentSubs.add(sub);
  ensureConnected();
  return () => {
    agentSubs.delete(sub);
    maybeClose();
  };
};

export const watchAgentMail = (onMail: AgentMailHandler, onConnect: ConnectHandler): (() => void) => {
  const sub: AgentMailSub = { onMail, onConnect };
  agentMailSubs.add(sub);
  ensureConnected();
  return () => {
    agentMailSubs.delete(sub);
    maybeClose();
  };
};
