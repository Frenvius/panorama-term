import type { AgentEvent } from '~/domain/interfaces/pty.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { Tile } from '~/domain/interfaces/workspace.interface';

export const muteWorkerNotifications = (tile: Tile | undefined): boolean =>
  tile?.type === 'term' && Boolean(tile.orchWorker || tile.runId);

const AGENT_EVENT_GRACE_MS = 5000;
const NOTIFY_IDLE_GRACE_MS = 10000;
const recent = new Map<string, { source: symbol; kind: NotifyKind; at: number }>();

interface NotifierArgs {
  tileId?: string;
  suppressed: () => boolean;
  notify: (kind: NotifyKind, text?: string, title?: string) => void;
  clear: () => void;
  focused?: () => boolean;
}

export const createAgentNotifier = ({ tileId, suppressed, notify, clear, focused }: NotifierArgs) => {
  const appFocused = focused ?? (() => document.hasFocus());
  const source = Symbol();
  const send: NotifierArgs['notify'] = (kind, text, title) => {
    if (tileId) {
      const now = Date.now();
      const previous = recent.get(tileId);
      if (previous && previous.source !== source && previous.kind === kind && now - previous.at < 1000) return;
      recent.set(tileId, { source, kind, at: now });
      setTimeout(() => { if (recent.get(tileId)?.at === now) recent.delete(tileId); }, 1000);
    }
    notify(kind, text, title);
  };
  let sawAgentEvents = false;
  let lastAgentEvent = 0;
  let lastNotify = 0;

  const onStatus = (prev: string | undefined, next: string, agent?: string): void => {
    if (suppressed() || sawAgentEvents) return;
    if (prev === 'busy' && next === 'idle') send('finished');
    else if (next === 'waiting' && agent !== 'codex') send('attention');
  };

  const onNotify = (title: string, body: string): void => {
    if (Date.now() - lastAgentEvent < AGENT_EVENT_GRACE_MS) return;
    if (!suppressed()) send('generic', body, title || undefined);
  };

  const onAgentEvent = (evt: AgentEvent): void => {
    sawAgentEvents = true;
    lastAgentEvent = Date.now();
    if (evt.event === 'prompt-submit') {
      clear();
      return;
    }
    if (suppressed()) return;
    if (evt.event === 'stop') {
      lastNotify = Date.now();
      const agent = evt.agent === 'pi' ? 'Pi' : evt.agent === 'codex' ? 'Codex' : evt.agent === 'antigravity' ? 'Antigravity' : 'Claude';
      send('finished', evt.response || `${agent} finished`);
      return;
    }
    if (evt.event === 'permission') {
      if (evt.agent === 'codex') return;
      lastNotify = Date.now();
      send('permission', [evt.toolName, evt.message].filter(Boolean).join(': ') || undefined);
      return;
    }
    if (evt.event === 'notification') {
      if (Date.now() - lastNotify < NOTIFY_IDLE_GRACE_MS) return;
      if (!appFocused()) send('idle', evt.message || undefined);
      return;
    }
    if (evt.event === 'compact') {
      lastNotify = Date.now();
      send('generic', undefined, 'Context compacted');
    }
  };

  return { onStatus, onNotify, onAgentEvent };
};
