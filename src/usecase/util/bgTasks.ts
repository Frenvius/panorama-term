import type { BackgroundShell, Ended, Subagent } from '~/domain/interfaces/pty.interface';

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

// Matches `codex`, `~/.local/bin/codex`, `"C:\...\codex.exe"`, `codex.cmd` in command position, so not `grep codex`;
// Windows wrappers carry an extension. Same pattern as the Claude mod's.
const CLI_RE =
  /(?:^|[;&|(]|\b(?:exec|npx|env|nohup|sudo|timeout\s+\S+)\b)\s*(?:\w+=\S*\s+)*(?:["'][^"']*[/\\]|["']|[^\s"';&|()]*[/\\])?(codex|gemini|agy|antigravity|opencode|kimi|cursor-agent|aider)(?:\.exe|\.cmd|\.ps1)?["']?(?=\s|$)/i;

// The agent CLI a shell command runs, if any.
export const cliOf = (command: string): string | undefined => CLI_RE.exec(command)?.[1]?.toLowerCase();

const detailKey = (t: BgTask) => JSON.stringify([t.description, t.label, t.model, t.tokens, t.activity]);

// Running and ended tasks of one kind come from the Claude mod; one missing from a later report, or reported ended, has ended.
// One that shows up again without an end was only missing from a transient report, so it is running.
// An ended one not seen running (it started and ended between two reads, or before this client connected) is added ended.
export const mergeTasks = (
  prev: BgTask[],
  live: BgTask[],
  now: number,
  ended: Ended<BgTask>[] = [],
  kind?: 'agent'
): BgTask[] => {
  const ends: Record<string, string> = Object.fromEntries(ended.map((t) => [t.id, t.status]));
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
  const prevIds = new Set(prev.map((t) => t.id));
  const added = [
    ...live.filter((t) => !prevIds.has(t.id)),
    ...ended.filter((t) => !prevIds.has(t.id) && !byId.has(t.id)).map((t) => ({ ...t, endedAt: now }))
  ];
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
// Finished tasks started up to this time were cleared or dropped over the limit. The mod keeps reporting
// the ones that ended, and they stay out.
const clearedKey = (tileId: string) => `panorama:tasks-cleared:${tileId}`;
const clearedAt = (tileId: string) => Number(getPref(clearedKey(tileId)) ?? 0);
const setCleared = (tileId: string, at: number) => setPref(clearedKey(tileId), String(Math.max(at, clearedAt(tileId))));

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

// Keeps every running task and the newest finished ones.
const trim = (list: BgTask[]) => {
  let finished = 0;
  const kept = list.filter((t) => !t.endedAt || ++finished <= SAVED_MAX);
  return kept.length === list.length ? list : kept;
};

const save = (tileId: string, all: BgTask[]) => {
  const list = trim(all);
  if (list !== all) setCleared(tileId, Math.max(...all.filter((t) => !list.includes(t)).map((t) => t.startedAt)));
  tasks.set(tileId, list);
  if (list.length) setPref(savedKey(tileId), JSON.stringify(list));
  else removePref(savedKey(tileId));
  notify();
};

// True when a running task that was not listed before showed up.
const report = (tileId: string, live: BgTask[], ended: Ended<BgTask>[] | undefined, kind?: 'agent'): boolean => {
  const prev = tasksOf(tileId);
  const cleared = clearedAt(tileId);
  const listed = new Set(prev.map((t) => t.id));
  const kept = ended?.filter((t) => t.startedAt > cleared || listed.has(t.id));
  const next = mergeTasks(prev, live, Date.now(), kept, kind);
  if (next === prev) return false;
  save(tileId, next);
  return live.some((t) => !listed.has(t.id));
};

export const reportShells = (tileId: string, shells: BackgroundShell[], ended?: Ended<BackgroundShell>[]): boolean =>
  report(tileId, shells, ended);

export const reportAgents = (tileId: string, agents: Subagent[], ended: Ended<Subagent>[] = []): boolean => {
  const now = Date.now();
  const task = (a: Subagent) => agentTask(a, now);
  return report(
    tileId,
    agents.map(task),
    ended.map((a) => ({ ...task(a), status: a.status })),
    'agent'
  );
};

// The terminal's session is gone: tasks it still listed as running ended with it.
export const endTasks = (tileId: string): void => {
  const list = tasksOf(tileId);
  if (!list.some((t) => !t.endedAt)) return;
  const now = Date.now();
  save(
    tileId,
    list.map((t) => (t.endedAt ? t : { ...t, endedAt: now, status: 'killed' }))
  );
};

export const clearFinished = (tileId: string): void => {
  setCleared(tileId, Date.now());
  save(
    tileId,
    tasksOf(tileId).filter((t) => !t.endedAt)
  );
};

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

let tileViews = 0;

// Whether the tasks open in a tile: set to, and a view that has tasks tiles is open.
export const tasksInTile = (): boolean => getTasksOpen() !== 'inline' && tileViews > 0;

export const onTasksTileRequest = (cb: (request: TasksTileRequest) => void): (() => void) => {
  const handler = (e: Event) => {
    e.preventDefault();
    cb((e as CustomEvent<TasksTileRequest>).detail);
  };
  window.addEventListener(REQUEST_EVENT, handler);
  tileViews++;
  return () => {
    window.removeEventListener(REQUEST_EVENT, handler);
    tileViews--;
  };
};
