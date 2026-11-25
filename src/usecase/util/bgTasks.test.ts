import { expect, test, mock } from 'bun:test';

const prefs = new Map<string, string>();
mock.module('~/usecase/util/prefs', () => ({
  getPref: (key: string) => prefs.get(key) ?? null,
  setPref: (key: string, value: string) => void prefs.set(key, value),
  removePref: (key: string) => void prefs.delete(key)
}));

mock.module('~/adapter/settings/settings.client', () => ({
  getSetting: <T>(_key: string, fallback: T): T => fallback,
  setSetting: async () => {}
}));

const { mergeTasks, placeTasksTile, reportShells, reportAgents, clearFinished, cliOf, TASKS_TILE } = await import('~/usecase/util/bgTasks');

const shell = (id: string) => ({ id, command: `run ${id}`, startedAt: 1 });

test('new shells come first and keep running', () => {
  const next = mergeTasks([], [shell('a'), shell('b')], 10);
  expect(next.map((t) => t.id)).toEqual(['a', 'b']);
  expect(next.every((t) => !t.endedAt)).toBe(true);
  expect(mergeTasks(next, [shell('c'), shell('a'), shell('b')], 20).map((t) => t.id)).toEqual(['c', 'a', 'b']);
});

test('a shell missing from the report ends once, at that time', () => {
  const running = mergeTasks([], [shell('a'), shell('b')], 10);
  const ended = mergeTasks(running, [shell('b')], 20);
  expect(ended.find((t) => t.id === 'a')?.endedAt).toBe(20);
  expect(ended.find((t) => t.id === 'b')?.endedAt).toBeUndefined();
  expect(mergeTasks(ended, [], 30).find((t) => t.id === 'a')?.endedAt).toBe(20);
});

test('a shell that reappears after a transient empty report is running again', () => {
  const running = mergeTasks([], [shell('a')], 10);
  const blip = mergeTasks(running, [], 20);
  expect(mergeTasks(blip, [shell('a')], 30)[0].endedAt).toBeUndefined();
});

test('a reported end finishes the task with its status, even while still listed', () => {
  const running = mergeTasks([], [shell('a'), shell('b')], 10);
  const next = mergeTasks(running, [shell('a'), shell('b')], 20, { a: 'failed' });
  expect(next.find((t) => t.id === 'a')).toMatchObject({ endedAt: 20, status: 'failed' });
  expect(next.find((t) => t.id === 'b')?.endedAt).toBeUndefined();
});

test('a status arriving after the task already ended keeps its end time', () => {
  const ended = mergeTasks(mergeTasks([], [shell('a')], 10), [], 20);
  expect(mergeTasks(ended, [], 30, { a: 'completed' })[0]).toMatchObject({ endedAt: 20, status: 'completed' });
});

test('a known status survives a report that no longer carries it', () => {
  const ended = mergeTasks(mergeTasks([], [shell('a')], 10), [], 20, { a: 'failed' });
  expect(mergeTasks(ended, [], 30)).toBe(ended);
});

test('an unchanged report keeps the same list, so nothing re-renders', () => {
  const running = mergeTasks([], [shell('a')], 10);
  expect(mergeTasks(running, [shell('a')], 20)).toBe(running);
});

const src = { x: 0, y: 0, width: 600, height: 400 };

test('a fixed side opens there even over other tiles', () => {
  const right = placeTasksTile(src, [{ x: 650, y: 0, width: 100, height: 100 }], 'right');
  expect(right.x).toBe(640);
  expect(placeTasksTile(src, [], 'left').x).toBe(-TASKS_TILE.width - 40);
  expect(placeTasksTile(src, [], 'below').y).toBe(440);
  expect(placeTasksTile(src, [], 'above').y).toBe(-TASKS_TILE.height - 40);
});

test('auto takes the first free side: right, below, left, above', () => {
  expect(placeTasksTile(src, [], 'auto')).toMatchObject({ x: 640, y: 0 });
  const rightTaken = [{ x: 650, y: 0, width: 300, height: 300 }];
  expect(placeTasksTile(src, rightTaken, 'auto')).toMatchObject({ x: 0, y: 440 });
});

test('auto falls back to the right when every side is taken', () => {
  const wall = [{ x: -2000, y: -2000, width: 5000, height: 1990 }, { x: -2000, y: 410, width: 5000, height: 2000 }, { x: -2000, y: 0, width: 1990, height: 400 }, { x: 610, y: 0, width: 2000, height: 400 }];
  expect(placeTasksTile(src, wall, 'auto')).toMatchObject({ x: 640, y: 0 });
});

test('tasks are saved per terminal; clearing drops only the finished ones', () => {
  reportShells('t1', [shell('a'), shell('b')]);
  reportShells('t1', [shell('b')], { a: 'completed' });
  const saved = (): [string, string | undefined][] => JSON.parse(prefs.get('panorama:tasks:t1') ?? '[]').map((t: { id: string; status?: string }) => [t.id, t.status]);
  expect(saved()).toEqual([['a', 'completed'], ['b', undefined]]);
  clearFinished('t1');
  expect(saved()).toEqual([['b', undefined]]);
  reportShells('t1', []);
  clearFinished('t1');
  expect(prefs.has('panorama:tasks:t1')).toBe(false);
});

const agent = (id: string, extra: Record<string, unknown> = {}) => ({ id, type: 'Explore', description: `find ${id}`, status: 'running', ...extra });

test('a running subagent picks up its new progress and activity', () => {
  const first = mergeTasks([], [{ id: 'a', kind: 'agent', command: 'Explore', startedAt: 1, activity: ['Read a.ts'] }], 10, {}, 'agent');
  const next = mergeTasks(first, [{ id: 'a', kind: 'agent', command: 'Explore', startedAt: 9, label: 'reading', activity: ['Read a.ts', 'Grep foo'] }], 20, {}, 'agent');
  expect(next[0].activity).toEqual(['Read a.ts', 'Grep foo']);
  expect(next[0].label).toBe('reading');
  expect(next[0].startedAt).toBe(1);
  expect(mergeTasks(next, [{ ...next[0] }], 30, {}, 'agent')).toBe(next);
});

test('a shell report leaves subagents alone', () => {
  const agents = mergeTasks([], [{ id: 'a', kind: 'agent', command: 'Explore', startedAt: 1 }], 10, {}, 'agent');
  const next = mergeTasks(agents, [shell('s')], 20);
  expect(next.find((t) => t.id === 'a')?.endedAt).toBeUndefined();
});

test('reporting tells when a new task starts', () => {
  expect(reportShells('fresh', [shell('x')])).toBe(true);
  expect(reportShells('fresh', [shell('x')])).toBe(false);
  expect(reportAgents('fresh', [agent('y')])).toBe(true);
});

test('cliOf finds the agent CLI a command runs', () => {
  expect(cliOf('codex exec -s workspace-write "do it"')).toBe('codex');
  expect(cliOf('cd repo && ~/.local/bin/codex exec x')).toBe('codex');
  expect(cliOf('timeout 400 gemini -p hi')).toBe('gemini');
  expect(cliOf('grep codex-notes.md file')).toBeUndefined();
  expect(cliOf('bun test')).toBeUndefined();
});
