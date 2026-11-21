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
