import type { RunSummary } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { watchOrchEvents } from '~/adapter/ui/ui.client';
import { listRuns, setCoordinator, listCoordinators, setRunCoordinator } from '~/adapter/orch/orch.client';

interface CoordinatorState {
  coordinators: string[];
  runs: RunSummary[];
  error: string | null;
}

let state: CoordinatorState = { coordinators: [], runs: [], error: null };

type Listener = () => void;
const listeners = new Set<Listener>();

const notify = (): void => {
  for (const fn of listeners) fn();
};

const fetchAll = (): void => {
  Promise.all([listCoordinators(), listRuns()])
    .then(([c, r]) => {
      state = { coordinators: c.tiles, runs: r.runs, error: null };
      notify();
    })
    .catch((err) => {
      state = { ...state, error: err instanceof Error ? err.message : String(err) };
      notify();
    });
};

const getSnapshot = (): CoordinatorState => state;

let wsUnsub: (() => void) | null = null;
let subscriberCount = 0;

const subscribe = (fn: Listener): (() => void) => {
  listeners.add(fn);
  subscriberCount++;
  if (subscriberCount === 1) {
    fetchAll();
    wsUnsub = watchOrchEvents(() => fetchAll(), fetchAll);
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

export const coordinatorActions = {
  toggle: (tileId: string): void => {
    const on = !state.coordinators.includes(tileId);
    setCoordinator(tileId, on).catch((err) => {
      state = { ...state, error: err instanceof Error ? err.message : String(err) };
      notify();
    });
  },
  attachToRun: (tileId: string, runId: string): void => {
    setRunCoordinator(runId, tileId).catch((err) => {
      state = { ...state, error: err instanceof Error ? err.message : String(err) };
      notify();
    });
  }
};

export interface CoordinatorHookResult {
  on: boolean;
  needsYou: boolean;
  toggle: () => void;
  error: string | null;
  label: string | null;
  run: RunSummary | null;
  allRuns: RunSummary[];
}

const runLabel = (summary: RunSummary): { label: string | null; needsYou: number } => {
  const count = (status: string) => summary.taskCounts[status] ?? 0;
  const running = count('dispatched') + count('verifying');
  const needsYou = count('failed') + count('blocked') + count('review') + (summary.openGates ?? 0);
  const parts: string[] = [];
  if (running > 0) parts.push(`${running} running`);
  if (needsYou > 0) parts.push(`${needsYou} needs you`);
  if (parts.length === 0 && count('pending') > 0) parts.push(`${count('pending')} queued`);
  if (parts.length === 0 && count('completed') > 0) parts.push('done');
  return { label: parts.length > 0 ? parts.join(' · ') : null, needsYou };
};

export const useOrchRuns = (): RunSummary[] => {
  const snap = React.useSyncExternalStore(subscribe, getSnapshot);
  return snap.runs;
};

export const useOrchCoordinator = (tileId: string): CoordinatorHookResult => {
  const snap = React.useSyncExternalStore(subscribe, getSnapshot);
  const toggle = React.useCallback(() => coordinatorActions.toggle(tileId), [tileId]);

  const on = snap.coordinators.includes(tileId);
  const run =
    snap.runs
      .filter((r) => r.run.coordinatorTile === tileId && (r.run.status === 'active' || r.run.status === 'paused'))
      .sort((a, b) => b.run.createdAt - a.run.createdAt)[0] ?? null;
  const { label, needsYou } = run ? runLabel(run) : { label: null, needsYou: 0 };

  return { on, run, label, toggle, error: snap.error, needsYou: needsYou > 0, allRuns: snap.runs };
};
