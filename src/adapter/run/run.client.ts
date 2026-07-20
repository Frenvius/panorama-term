import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { RunStatus } from '~/domain/interfaces/run.interface';

const SIDECAR_HTTP = 'http://127.0.0.1:9777';
const POLL_MS = 3000;
const NONE: RunStatus = { state: 'none' };

export type RunKind = 'run' | 'build';

export const detectRunCommands = (path: string): Promise<string[]> =>
  invoke<string[]>('run_commands', { path }).catch(() => [] as string[]);

export const fetchRunStatus = (tile: string, kind: RunKind = 'run'): Promise<RunStatus> =>
  fetch(`${SIDECAR_HTTP}/run/status?kind=${kind}&tile=${encodeURIComponent(tile)}`)
    .then((r) => r.json() as Promise<RunStatus>)
    .catch(() => NONE);

export const startRun = (cwd: string, cmd: string, tile: string, kind: RunKind = 'run'): Promise<RunStatus> =>
  fetch(
    `${SIDECAR_HTTP}/run/start?kind=${kind}&cwd=${encodeURIComponent(cwd)}&cmd=${encodeURIComponent(cmd)}&tile=${encodeURIComponent(tile)}`
  )
    .then((r) => r.json() as Promise<RunStatus>)
    .catch(() => NONE);

export const stopRun = (tile: string, hard = false, kind: RunKind = 'run'): Promise<unknown> =>
  fetch(`${SIDECAR_HTTP}/run/stop?kind=${kind}&tile=${encodeURIComponent(tile)}${hard ? '&hard=1' : ''}`).catch(
    () => undefined
  );

type Listener = (s: RunStatus) => void;

interface Watcher {
  timer: ReturnType<typeof setInterval>;
  listeners: Set<Listener>;
  last: RunStatus;
}

const watchers = new Map<string, Watcher>();

const poll = (tile: string, kind: RunKind): void => {
  const key = `${kind}:${tile}`;
  void fetchRunStatus(tile, kind).then((status) => {
    const w = watchers.get(key);
    if (!w) return;
    w.last = status;
    for (const fn of w.listeners) fn(status);
  });
};

export const watchRun = (tile: string, fn: Listener, kind: RunKind = 'run'): (() => void) => {
  const key = `${kind}:${tile}`;
  let w = watchers.get(key);
  if (!w) {
    w = { timer: setInterval(() => poll(tile, kind), POLL_MS), listeners: new Set(), last: NONE };
    watchers.set(key, w);
    poll(tile, kind);
  } else {
    fn(w.last);
  }
  w.listeners.add(fn);
  return () => {
    const cur = watchers.get(key);
    if (!cur) return;
    cur.listeners.delete(fn);
    if (cur.listeners.size === 0) {
      clearInterval(cur.timer);
      watchers.delete(key);
    }
  };
};

export const refreshRun = (tile: string, kind: RunKind = 'run'): void => poll(tile, kind);

export const watchRunManifests = (cwd: string, onChange: () => void): (() => void) => {
  let alive = true;
  let watchId: number | null = null;
  void invoke<number>('run_watch_manifests', { path: cwd })
    .then((id) => {
      if (!alive) void invoke('run_unwatch_manifests', { id });
      else watchId = id;
    })
    .catch(() => undefined);
  const off = listen<{ path: string }>('run:manifests', (e) => {
    if (e.payload.path === cwd) onChange();
  });
  return () => {
    alive = false;
    if (watchId !== null) void invoke('run_unwatch_manifests', { id: watchId });
    void off.then((un) => un());
  };
};
