import { expect, test } from 'bun:test';

import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';
import type { RunSummary } from '~/domain/interfaces/orch.interface';

import { frameBoards, liveBoards, orphanBoards } from '~/usecase/util/boardProject';

const run = (id: string, repo: string, coordinatorTile?: string, status = 'active'): RunSummary =>
  ({ run: { id, repo, coordinatorTile, status } } as RunSummary);

test('frame boards match normalized repos or coordinators once, excluding finished runs', () => {
  const runs = [run('repo', 'D:/ONE/'), run('coordinator', 'D:/two', 'tile'), run('both', 'd:\\one', 'tile'), run('finished', 'd:/one', 'tile', 'completed'), run('other', 'd:/other')];
  expect(frameBoards(runs, 'd:/one', ['tile']).map((s) => s.run.id)).toEqual(['repo', 'coordinator', 'both']);
  expect(frameBoards(runs, undefined, ['tile']).map((s) => s.run.id)).toEqual(['coordinator', 'both']);
});

test('orphan boards have neither a matching frame nor a coordinator on the canvas', () => {
  const runs = [run('frame', 'd:/ONE/'), run('tile', 'd:/two', 'tile'), run('orphan', 'd:/three'), run('missing', 'd:/four', 'gone', 'paused'), run('finished', 'd:/five', undefined, 'cancelled')];
  const frames = [{ path: 'D:\\one' }] as Frame[];
  const tiles = [{ id: 'tile' }] as Tile[];
  expect(orphanBoards(runs, frames, tiles).map((s) => s.run.id)).toEqual(['orphan', 'missing']);
});

test('live boards require attention and only keep recent failures, sorted by activity', () => {
  const now = 200_000_000;
  const summary = (id: string, activity: number): RunSummary => ({
    ...run(id, id), run: { ...run(id, id).run, updatedAt: activity },
    taskCounts: {}, openGates: 0, runningDispatches: 0, latestActivity: activity
  });
  const stale = summary('stale', 1);
  const running = { ...summary('running', 2), runningDispatches: 1 };
  const review = { ...summary('review', 3), taskCounts: { review: 1 } };
  const gate = { ...summary('gate', 4), openGates: 1 };
  const blocked = { ...summary('blocked', 5), taskCounts: { blocked: 1 } };
  const failed = { ...summary('failed', 6), taskCounts: { failed: 1 } };
  const recent = { ...summary('recent', 1), latestActivity: now - 1, taskCounts: { failed: 1 } };
  const recentIdle = { ...summary('idle', now - 1) };
  const recentBlocked = { ...summary('recentBlocked', now - 2), taskCounts: { blocked: 1 } };
  const finished = { ...recent, run: { ...recent.run, id: 'finished', status: 'completed' as const } };
  expect(liveBoards([stale, running, review, gate, blocked, failed, recent, recentIdle, recentBlocked, finished], now).map((s) => s.run.id))
    .toEqual(['recent', 'recentBlocked', 'gate', 'review', 'running']);
});
