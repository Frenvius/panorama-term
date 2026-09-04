import type { AgentEvent } from '~/domain/interfaces/pty.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';

const AGENT_EVENT_GRACE_MS = 5000;
const NOTIFY_IDLE_GRACE_MS = 10000;

interface NotifierArgs {
  suppressed: () => boolean;
  notify: (kind: NotifyKind, text?: string, title?: string) => void;
  clear: () => void;
  focused?: () => boolean;
}

export const createAgentNotifier = ({ suppressed, notify, clear, focused }: NotifierArgs) => {
  const appFocused = focused ?? (() => document.hasFocus());
  let sawAgentEvents = false;
  let lastAgentEvent = 0;
  let lastNotify = 0;

  const onStatus = (prev: string | undefined, next: string): void => {
    if (suppressed() || sawAgentEvents) return;
    if (prev === 'busy' && next === 'idle') notify('finished');
    else if (next === 'waiting') notify('attention');
  };

  const onNotify = (title: string, body: string): void => {
    if (Date.now() - lastAgentEvent < AGENT_EVENT_GRACE_MS) return;
    if (!suppressed()) notify('generic', body, title || undefined);
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
      const agent = evt.agent === 'pi' ? 'Pi' : 'Claude';
      notify('finished', evt.response || `${agent} finished`);
      return;
    }
    if (evt.event === 'permission') {
      lastNotify = Date.now();
      notify('permission', [evt.toolName, evt.message].filter(Boolean).join(': ') || undefined);
      return;
    }
    if (evt.event === 'notification') {
      if (Date.now() - lastNotify < NOTIFY_IDLE_GRACE_MS) return;
      if (!appFocused()) notify('idle', evt.message || undefined);
      return;
    }
    if (evt.event === 'compact') {
      lastNotify = Date.now();
      notify('generic', undefined, 'Context compacted');
    }
  };

  return { onStatus, onNotify, onAgentEvent };
};
