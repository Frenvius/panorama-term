import type { BackgroundShell, Subagent } from '~/domain/interfaces/pty.interface';

import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { getPref, setPref, removePref } from '~/usecase/util/prefs';

// A shell the agent runs (in the background, or another agent CLI in the foreground) or one of its subagents.
export interface BgTask extends BackgroundShell {
  // Absent for shells. An agent's `command` is its subagent type.
  kind?: 'agent';
  endedAt?: number;
  // completed, failed or killed, when the agent reported how it ended.
  status?: string;
  label?: string;
  model?: string;
  tokens?: number;
  activity?: string[];
}

export type TasksOpen = 'inline' | 'auto' | 'right' | 'left' | 'above' | 'below';
export type TasksLink = 'dashed' | 'solid' | 'highlight' | 'none';
export type TasksDetail = 'compact' | 'normal' | 'full';
type Side = Exclude<TasksOpen, 'inline' | 'auto'>;
interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const TASKS_OPEN_KEY = 'tasksOpen';
export const TASKS_LINK_KEY = 'tasksLink';
export const TASKS_DETAIL_KEY = 'tasksDetail';
export const TASKS_AUTO_KEY = 'tasksAutoOpen';
export const TASKS_TILE = { width: 460, height: 360 };
const GAP = 40;
const SAVED_MAX = 30;
const AUTO_ORDER: Side[] = ['right', 'below', 'left', 'above'];
const EMPTY: BgTask[] = [];
const REQUEST_EVENT = 'tasks:open';

const CLI_RE = /(?:^|[\s;&|(/])(codex|gemini|agy|antigravity|opencode|kimi|cursor-agent|aider)(?=\s|$)/;

// The agent CLI a shell command runs, if any.
export const cliOf = (command: string): string | undefined => CLI_RE.exec(command)?.[1];

const detailKey = (t: BgTask) => JSON.stringify([t.description, t.label, t.model, t.tokens, t.activity]);

// Running tasks of one kind come from the Claude mod; one missing from a later report, or with a reported end, has ended.
// One that shows up again without an end was only missing from a transient report, so it is running.
export const mergeTasks = (prev: BgTask[], live: BgTask[], now: number, ends: Record<string, string> = {}, kind?: 'agent'): BgTask[] => {
  const byId = new Map(live.map((t) => [t.id, t]));
  let changed = false;
  const kept = prev.map((t) => {
    if (t.kind !== kind) return t;
    const status = ends[t.id] ?? t.status;
    const fresh = byId.get(t.id);
    const running = Boolean(fresh) && !status;
    let next = fresh && running && detailKey(fresh) !== detailKey(t) ? { ...t, ...fresh, startedAt: t.startedAt } : t;
    if (running === !next.endedAt && status === next.status) {
      changed ||= next !== t;
      return next;
    }
    next = running ? { ...next, endedAt: undefined } : { ...next, endedAt: next.endedAt ?? now, status };
    changed = true;
    return next;
  });
  const known = new Set(prev.map((t) => t.id));
  const added = live.filter((t) => !known.has(t.id));
  return added.length || changed ? [...added, ...kept] : prev;
};

export const agentTask = (a: Subagent, now: number): BgTask => ({
  id: a.id,
  kind: 'agent',
  command: a.type,
  description: a.description,
  startedAt: a.startedAt ?? now,
  label: a.label,
  model: a.model,
  tokens: a.tokens,
  activity: a.activity
});

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

// True when a task started that was not listed before.
const report = (tileId: string, live: BgTask[], ends: Record<string, string> | undefined, kind?: 'agent'): boolean => {
  const prev = tasksOf(tileId);
  const next = mergeTasks(prev, live, Date.now(), ends, kind);
  if (next === prev) return false;
  save(tileId, next);
  return next.length > prev.length;
};

export const reportShells = (tileId: string, shells: BackgroundShell[], ends?: Record<string, string>): boolean =>
  report(tileId, shells, ends);

export const reportAgents = (tileId: string, agents: Subagent[], ends?: Record<string, string>): boolean => {
  const now = Date.now();
  return report(tileId, agents.map((a) => agentTask(a, now)), ends, 'agent');
};

export const clearFinished = (tileId: string): void => save(tileId, tasksOf(tileId).filter((t) => !t.endedAt));

export const useBgTasks = (tileId: string): BgTask[] => React.useSyncExternalStore(subscribe, () => tasksOf(tileId));

export const getTasksOpen = (): TasksOpen => getSetting(TASKS_OPEN_KEY, 'inline');
export const getTasksLink = (): TasksLink => getSetting(TASKS_LINK_KEY, 'dashed');

export const getTasksDetail = (): TasksDetail => getSetting(TASKS_DETAIL_KEY, 'normal');
export const getTasksAuto = (): boolean => getSetting(TASKS_AUTO_KEY, false);

export const setTasksOpen = (value: TasksOpen): void => void setSetting(TASKS_OPEN_KEY, value);
export const setTasksAuto = (value: boolean): void => void setSetting(TASKS_AUTO_KEY, value);

export const setTasksDetail = (value: TasksDetail): void => {
  void setSetting(TASKS_DETAIL_KEY, value);
  notify();
};

export const useTasksDetail = (): TasksDetail => React.useSyncExternalStore(subscribe, getTasksDetail);

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

interface TasksTileRequest {
  sourceId: string;
  // False when the tile opens on its own: it must not take focus or leave fullscreen.
  focus: boolean;
}

// True when a view opened the tile; views without tasks tiles (project view) leave it unhandled.
export const requestTasksTile = (sourceId: string, focus = true): boolean =>
  !window.dispatchEvent(new CustomEvent<TasksTileRequest>(REQUEST_EVENT, { detail: { sourceId, focus }, cancelable: true }));

export const onTasksTileRequest = (cb: (request: TasksTileRequest) => void): (() => void) => {
  const handler = (e: Event) => {
    e.preventDefault();
    cb((e as CustomEvent<TasksTileRequest>).detail);
  };
  window.addEventListener(REQUEST_EVENT, handler);
  return () => window.removeEventListener(REQUEST_EVENT, handler);
};
