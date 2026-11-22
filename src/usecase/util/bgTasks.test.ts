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

const { mergeShells, placeTasksTile, reportShells, clearFinished, TASKS_TILE } = await import('~/usecase/util/bgTasks');

const shell = (id: string) => ({ id, command: `run ${id}`, startedAt: 1 });

test('new shells come first and keep running', () => {
  const next = mergeShells([], [shell('a'), shell('b')], 10);
  expect(next.map((t) => t.id)).toEqual(['a', 'b']);
  expect(next.every((t) => !t.endedAt)).toBe(true);
  expect(mergeShells(next, [shell('c'), shell('a'), shell('b')], 20).map((t) => t.id)).toEqual(['c', 'a', 'b']);
});

test('a shell missing from the report ends once, at that time', () => {
  const running = mergeShells([], [shell('a'), shell('b')], 10);
  const ended = mergeShells(running, [shell('b')], 20);
  expect(ended.find((t) => t.id === 'a')?.endedAt).toBe(20);
  expect(ended.find((t) => t.id === 'b')?.endedAt).toBeUndefined();
  expect(mergeShells(ended, [], 30).find((t) => t.id === 'a')?.endedAt).toBe(20);
});

test('a shell that reappears after a transient empty report is running again', () => {
  const running = mergeShells([], [shell('a')], 10);
  const blip = mergeShells(running, [], 20);
  expect(mergeShells(blip, [shell('a')], 30)[0].endedAt).toBeUndefined();
});

test('a reported end finishes the task with its status, even while still listed', () => {
  const running = mergeShells([], [shell('a'), shell('b')], 10);
  const next = mergeShells(running, [shell('a'), shell('b')], 20, { a: 'failed' });
  expect(next.find((t) => t.id === 'a')).toMatchObject({ endedAt: 20, status: 'failed' });
  expect(next.find((t) => t.id === 'b')?.endedAt).toBeUndefined();
});

test('a status arriving after the task already ended keeps its end time', () => {
  const ended = mergeShells(mergeShells([], [shell('a')], 10), [], 20);
  expect(mergeShells(ended, [], 30, { a: 'completed' })[0]).toMatchObject({ endedAt: 20, status: 'completed' });
});

test('an unchanged report keeps the same list, so nothing re-renders', () => {
  const running = mergeShells([], [shell('a')], 10);
  expect(mergeShells(running, [shell('a')], 20)).toBe(running);
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

test('finished tasks are saved per terminal and cleared with the list', () => {
  reportShells('t1', [shell('a'), shell('b')]);
  reportShells('t1', [shell('b')], { a: 'completed' });
  const saved = JSON.parse(prefs.get('panorama:tasks:t1') ?? '[]');
  expect(saved.map((t: { id: string; status?: string }) => [t.id, t.status])).toEqual([['a', 'completed']]);
  clearFinished('t1');
  expect(prefs.has('panorama:tasks:t1')).toBe(false);
});
