import type { RunDetail, RunSummary } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { watchOrchEvents } from '~/adapter/ui/ui.client';
import { listRuns, showRun } from '~/adapter/orch/orch.client';

export interface OrchState {
  runs: RunSummary[];
  details: Map<string, RunDetail>;
  selectedRunId: string | null;
  error: string | null;
}

let state: OrchState = { runs: [], details: new Map(), selectedRunId: null, error: null };

type Listener = () => void;
const listeners = new Set<Listener>();

const notify = (): void => {
  for (const fn of listeners) fn();
};

const setError = (err: unknown): void => {
  state = { ...state, error: err instanceof Error ? err.message : String(err) };
  notify();
};

const updateRuns = (runs: RunSummary[]): void => {
  state = { ...state, runs, error: null };
  notify();
};

const updateDetail = (runId: string, detail: RunDetail): void => {
  const details = new Map(state.details);
  details.set(runId, detail);
  state = { ...state, details };
  notify();
};

const getSnapshot = (): OrchState => state;

const fetchList = (): void => {
  listRuns()
    .then(({ runs }) => updateRuns(runs))
    .catch(setError);
};

const debounceTimers = new Map<string, ReturnType<typeof setTimeout>>();

const fetchRunDebounced = (runId: string): void => {
  const existing = debounceTimers.get(runId);
  if (existing) clearTimeout(existing);
  debounceTimers.set(
    runId,
    setTimeout(() => {
      debounceTimers.delete(runId);
      fetchList();
      const shouldFetch = state.selectedRunId === runId || state.details.has(runId);
      if (!shouldFetch) return;
      showRun(runId)
        .then((detail) => updateDetail(runId, detail))
        .catch(setError);
    }, 200)
  );
};

export const orchActions = {
  selectRun: (runId: string | null): void => {
    state = { ...state, selectedRunId: runId };
    notify();
    if (runId && !state.details.has(runId)) {
      showRun(runId)
        .then((detail) => updateDetail(runId, detail))
        .catch(setError);
    }
  }
};

let wsUnsub: (() => void) | null = null;
let subscriberCount = 0;

export const subscribeOrch = (fn: Listener): (() => void) => {
  listeners.add(fn);
  subscriberCount++;
  if (subscriberCount === 1) {
    fetchList();
    wsUnsub = watchOrchEvents(fetchRunDebounced, fetchList);
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

export const useOrchRuns = (): OrchState & { selectRun: (id: string | null) => void } => {
  const snap = React.useSyncExternalStore(subscribeOrch, getSnapshot);
  return { ...snap, selectRun: orchActions.selectRun };
};
