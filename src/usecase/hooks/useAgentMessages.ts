import type { AgentMessage } from '~/domain/interfaces/agents.interface';

import React from 'react';

import { watchAgentMail } from '~/adapter/ui/ui.client';
import { listMessages } from '~/adapter/agents/agents.client';

interface MessagesState {
  messages: AgentMessage[];
  userUnread: number;
  error: string | null;
}

interface PartyStore {
  state: MessagesState;
  listeners: Set<() => void>;
  count: number;
  wsUnsub: (() => void) | null;
  debounce: ReturnType<typeof setTimeout> | undefined;
}

const stores = new Map<string, PartyStore>();
const subscribeFns = new Map<string, (fn: () => void) => () => void>();
const snapshotFns = new Map<string, () => MessagesState>();

const emptyState = (): MessagesState => ({ messages: [], userUnread: 0, error: null });

const notify = (key: string): void => {
  const s = stores.get(key);
  if (!s) return;
  for (const fn of s.listeners) fn();
};

const fetchFor = (key: string): void => {
  const opts = key === 'user' ? { party: 'user' as const, limit: 50 } : { agent: key, limit: 50 };
  listMessages(opts)
    .then((r) => {
      const s = stores.get(key);
      if (!s) return;
      s.state = { messages: r.messages, userUnread: r.userUnread, error: null };
      notify(key);
    })
    .catch((err: unknown) => {
      const s = stores.get(key);
      if (!s) return;
      s.state = { ...s.state, error: err instanceof Error ? err.message : String(err) };
      notify(key);
    });
};

const debouncedFetch = (key: string): void => {
  const s = stores.get(key);
  if (!s) return;
  clearTimeout(s.debounce);
  s.debounce = setTimeout(() => fetchFor(key), 200);
};

const mailKeyFor = (party: string): string | null => {
  if (party === 'user') return 'user';
  if (party.startsWith('agent:')) return party.slice(6);
  return null;
};

const buildSubscribeFn = (key: string): (fn: () => void) => () => void =>
  (fn: () => void): (() => void) => {
    let s = stores.get(key);
    if (!s) {
      s = { state: emptyState(), listeners: new Set(), count: 0, wsUnsub: null, debounce: undefined };
      stores.set(key, s);
    }
    s.listeners.add(fn);
    s.count++;
    if (s.count === 1) {
      fetchFor(key);
      s.wsUnsub = watchAgentMail(
        (parties) => {
          for (const p of parties) {
            if (mailKeyFor(p) === key) debouncedFetch(key);
          }
        },
        () => fetchFor(key),
      );
    }
    return () => {
      const store = stores.get(key);
      if (!store) return;
      store.listeners.delete(fn);
      store.count--;
      if (store.count === 0) {
        clearTimeout(store.debounce);
        store.wsUnsub?.();
        store.wsUnsub = null;
        stores.delete(key);
        subscribeFns.delete(key);
        snapshotFns.delete(key);
      }
    };
  };

const getSubscribeFn = (key: string): (fn: () => void) => () => void => {
  if (!subscribeFns.has(key)) subscribeFns.set(key, buildSubscribeFn(key));
  return subscribeFns.get(key)!;
};

const getSnapshotFn = (key: string): () => MessagesState => {
  if (!snapshotFns.has(key)) snapshotFns.set(key, () => stores.get(key)?.state ?? emptyState());
  return snapshotFns.get(key)!;
};

export const useAgentMessages = (party: string): MessagesState => {
  const sub = React.useCallback(getSubscribeFn(party), [party]);
  const snap = React.useCallback(getSnapshotFn(party), [party]);
  return React.useSyncExternalStore(sub, snap);
};

export const refreshMessages = (key: string): void => {
  if (stores.has(key)) fetchFor(key);
};
