import type { BackgroundShell } from '~/domain/interfaces/pty.interface';

import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { getPref, setPref, removePref } from '~/usecase/util/prefs';

export interface BgTask extends BackgroundShell {
  endedAt?: number;
  // completed, failed or killed, when the agent reported how it ended.
  status?: string;
}

export type TasksOpen = 'inline' | 'auto' | 'right' | 'left' | 'above' | 'below';
export type TasksLink = 'dashed' | 'solid' | 'highlight' | 'none';
type Side = Exclude<TasksOpen, 'inline' | 'auto'>;
interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const TASKS_OPEN_KEY = 'tasksOpen';
export const TASKS_LINK_KEY = 'tasksLink';
export const TASKS_TILE = { width: 460, height: 360 };
const GAP = 40;
const SAVED_MAX = 30;
const AUTO_ORDER: Side[] = ['right', 'below', 'left', 'above'];
const EMPTY: BgTask[] = [];
const REQUEST_EVENT = 'tasks:open';

// Running shells come from the Claude mod; a shell missing from a later report, or with a reported end, has ended.
// One that shows up again without an end was only missing from a transient report, so it is running.
export const mergeShells = (prev: BgTask[], shells: BackgroundShell[], now: number, ends: Record<string, string> = {}): BgTask[] => {
  const live = new Set(shells.map((s) => s.id));
  let changed = false;
  const kept = prev.map((t) => {
    const status = ends[t.id] ?? t.status;
    const running = live.has(t.id) && !status;
    if (running === !t.endedAt && status === t.status) return t;
    changed = true;
    return running ? { ...t, endedAt: undefined } : { ...t, endedAt: t.endedAt ?? now, status };
  });
  const known = new Set(prev.map((t) => t.id));
  const added = shells.filter((s) => !known.has(s.id));
  return added.length || changed ? [...added, ...kept] : prev;
};

const tasks = new Map<string, BgTask[]>();
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((l) => l());
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => void listeners.delete(l);
};

const savedKey = (tileId: string) => `panorama:tasks:${tileId}`;

// Tasks outlive a restart; one that ended while the app was closed shows as finished on the next report.
const tasksOf = (tileId: string): BgTask[] => {
  let list = tasks.get(tileId);
  if (!list) {
    try {
      list = JSON.parse(getPref(savedKey(tileId)) ?? '[]') as BgTask[];
    } catch {
      list = EMPTY;
    }
    tasks.set(tileId, list);
  }
  return list;
};

const save = (tileId: string, list: BgTask[]) => {
  tasks.set(tileId, list);
  const kept = list.slice(0, SAVED_MAX);
  if (kept.length) setPref(savedKey(tileId), JSON.stringify(kept));
  else removePref(savedKey(tileId));
  notify();
};

export const reportShells = (tileId: string, shells: BackgroundShell[], ends?: Record<string, string>): void => {
  const prev = tasksOf(tileId);
  const next = mergeShells(prev, shells, Date.now(), ends);
  if (next !== prev) save(tileId, next);
};

export const clearFinished = (tileId: string): void => save(tileId, tasksOf(tileId).filter((t) => !t.endedAt));

export const useBgTasks = (tileId: string): BgTask[] => React.useSyncExternalStore(subscribe, () => tasksOf(tileId));

export const getTasksOpen = (): TasksOpen => getSetting(TASKS_OPEN_KEY, 'inline');
export const getTasksLink = (): TasksLink => getSetting(TASKS_LINK_KEY, 'dashed');

export const setTasksOpen = (value: TasksOpen): void => void setSetting(TASKS_OPEN_KEY, value);

export const setTasksLink = (value: TasksLink): void => {
  void setSetting(TASKS_LINK_KEY, value);
  notify();
};

export const useTasksLink = (): TasksLink => React.useSyncExternalStore(subscribe, getTasksLink);

const overlaps = (a: Rect, b: Rect) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

const besideOf = (src: Rect, side: Side): Rect => {
  const { width, height } = TASKS_TILE;
  if (side === 'left') return { x: src.x - width - GAP, y: src.y, width, height };
  if (side === 'above') return { x: src.x, y: src.y - height - GAP, width, height };
  if (side === 'below') return { x: src.x, y: src.y + src.height + GAP, width, height };
  return { x: src.x + src.width + GAP, y: src.y, width, height };
};

// Where a tasks tile opens next to its terminal; 'auto' takes the first free side and never moves other tiles.
export const placeTasksTile = (src: Rect, others: Rect[], open: Exclude<TasksOpen, 'inline'>): Rect => {
  if (open !== 'auto') return besideOf(src, open);
  const free = AUTO_ORDER.map((side) => besideOf(src, side)).find((r) => !others.some((o) => overlaps(r, o)));
  return free ?? besideOf(src, 'right');
};

// True when a view opened the tile; views without tasks tiles (project view) leave it unhandled.
export const requestTasksTile = (sourceId: string): boolean =>
  !window.dispatchEvent(new CustomEvent(REQUEST_EVENT, { detail: sourceId, cancelable: true }));

export const onTasksTileRequest = (cb: (sourceId: string) => void): (() => void) => {
  const handler = (e: Event) => {
    e.preventDefault();
    cb((e as CustomEvent<string>).detail);
  };
  window.addEventListener(REQUEST_EVENT, handler);
  return () => window.removeEventListener(REQUEST_EVENT, handler);
};
