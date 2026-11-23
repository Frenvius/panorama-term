import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';
import type { RunSummary } from '~/domain/interfaces/orch.interface';

import { frameOf } from '~/usecase/util/frame';
import { normalizeRepo } from '~/usecase/util/orch';

export type BoardProject =
  | { kind: 'loose' }
  | { kind: 'noFolder'; frameId: string }
  | { kind: 'project'; frameId: string; repo: string };

export const tileBoardProject = (frames: Frame[], tiles: Tile[], tileId: string): BoardProject => {
  const tile = tiles.find((t) => t.id === tileId);
  if (!tile) return { kind: 'loose' };
  const frame = frameOf(frames, tile);
  if (!frame) return { kind: 'loose' };
  if (frame.path) return { kind: 'project', frameId: frame.id, repo: frame.path };
  return { kind: 'noFolder', frameId: frame.id };
};

export const projectBoards = (runs: RunSummary[], repo: string): RunSummary[] => {
  const norm = normalizeRepo(repo);
  return runs.filter(
    (s) => normalizeRepo(s.run.repo) === norm && s.run.status !== 'completed' && s.run.status !== 'cancelled'
  );
};

export const frameBoards = (runs: RunSummary[], repo: string | undefined, tileIds: string[]): RunSummary[] => {
  const norm = repo ? normalizeRepo(repo) : '';
  return runs.filter(({ run }) =>
    (run.status === 'active' || run.status === 'paused') &&
    ((norm !== '' && normalizeRepo(run.repo) === norm) || (run.coordinatorTile != null && tileIds.includes(run.coordinatorTile)))
  );
};

export const orphanBoards = (runs: RunSummary[], frames: Frame[], tiles: Tile[]): RunSummary[] =>
  runs.filter(({ run }) =>
    (run.status === 'active' || run.status === 'paused') &&
    !frames.some((frame) => frame.path && normalizeRepo(frame.path) === normalizeRepo(run.repo)) &&
    !tiles.some((tile) => tile.id === run.coordinatorTile)
  );

export const liveBoards = (runs: RunSummary[], now: number): RunSummary[] =>
  runs.filter((s) =>
    (s.run.status === 'active' || s.run.status === 'paused') &&
    (s.runningDispatches > 0 || s.openGates > 0 || s.taskCounts.review > 0 ||
      (['blocked', 'failed'].some((status) => s.taskCounts[status] > 0) &&
        Math.max(s.run.updatedAt, s.latestActivity ?? 0) >= now - 24 * 60 * 60 * 1000))
  ).sort((a, b) => Math.max(b.run.updatedAt, b.latestActivity ?? 0) - Math.max(a.run.updatedAt, a.latestActivity ?? 0));
