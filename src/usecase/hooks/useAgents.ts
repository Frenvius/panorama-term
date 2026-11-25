import type { PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';

import { listAgents } from '~/adapter/agents/agents.client';
import { watchAgentEvents } from '~/adapter/ui/ui.client';

interface AgentsState {
  agents: PersistentAgent[];
  restartNeededIds: Set<string>;
  error: string | null;
}

let state: AgentsState = { agents: [], restartNeededIds: new Set(), error: null };
const restartMarks = new Map<string, number>();

type Listener = () => void;
const listeners = new Set<Listener>();

const notify = (): void => {
  for (const fn of listeners) fn();
};

const fetchAgents = (): void => {
  listAgents()
    .then((r) => {
      for (const a of r.agents) {
        const markedAt = restartMarks.get(a.id);
        if (markedAt !== undefined && (!a.live || (a.lastStartedAt ?? 0) > markedAt)) restartMarks.delete(a.id);
      }
      state = { agents: r.agents, restartNeededIds: new Set(restartMarks.keys()), error: null };
      notify();
    })
    .catch((err) => {
      state = { ...state, error: err instanceof Error ? err.message : String(err) };
      notify();
    });
};

let debounceTimer: ReturnType<typeof setTimeout> | undefined;
const debouncedFetch = (): void => {
  clearTimeout(debounceTimer);
  debounceTimer = setTimeout(fetchAgents, 200);
};

const getSnapshot = (): AgentsState => state;

let wsUnsub: (() => void) | null = null;
let subscriberCount = 0;

const subscribe = (fn: Listener): (() => void) => {
  listeners.add(fn);
  subscriberCount++;
  if (subscriberCount === 1) {
    fetchAgents();
    wsUnsub = watchAgentEvents(debouncedFetch, fetchAgents);
  }
  return () => {
    listeners.delete(fn);
    subscriberCount--;
    if (subscriberCount === 0 && wsUnsub) {
      wsUnsub();
      wsUnsub = null;
    }
  };
};

export const agentActions = {
  markRestartNeeded: (id: string): void => {
    restartMarks.set(id, Date.now());
    state = { ...state, restartNeededIds: new Set(restartMarks.keys()) };
    notify();
  },
  refresh: (): void => {
    fetchAgents();
  }
};

export const useAgents = (): AgentsState => React.useSyncExternalStore(subscribe, getSnapshot);
