import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';

import { planGroupFrame, placeTileInFrame, findFreeRect } from '~/usecase/util/frameLayout';
import { FRAME_COLOR, FRAME_WIDTH, FRAME_HEIGHT } from '~/usecase/util/constants';

export interface GroupOps {
  tiles: Tile[];
  frames: Frame[];
  createFrame: (frame: Omit<Frame, 'id'>) => string;
  patchFrame: (id: string, patch: Partial<Omit<Frame, 'id'>>) => void;
  placeTiles: (moves: { id: string; x: number; y: number }[]) => void;
}

interface FixGroupFrameArgs {
  ops: GroupOps;
  group: ProjectGroup;
  near: { x: number; y: number };
}

export const fixGroupFrame = ({ ops, group, near }: FixGroupFrameArgs): string => {
  const { tiles, frames, createFrame, placeTiles } = ops;
  const groupTiles = [...group.tiles, ...group.outside];

  if (groupTiles.length === 0) {
    const obstacles = [
      ...tiles.map((t) => ({ x: t.x, y: t.y, width: t.width, height: t.height })),
      ...frames.map((f) => ({ x: f.x, y: f.y, width: f.width, height: f.height }))
    ];
    const { x, y } = findFreeRect({ width: FRAME_WIDTH, height: FRAME_HEIGHT, near, obstacles });
    return createFrame({ x, y, width: FRAME_WIDTH, height: FRAME_HEIGHT, title: group.title, color: FRAME_COLOR, path: group.path ?? undefined });
  }

  const result = planGroupFrame({ groupTiles, allTiles: tiles, frames });

  if (result.kind === 'gather') {
    placeTiles(result.moves);
  }

  return createFrame({ ...result.rect, title: group.title, color: FRAME_COLOR, path: group.path ?? undefined });
};

interface MoveTileIntoFrameArgs {
  ops: GroupOps;
  tile: Tile;
  frameId: string;
}

export const moveTileIntoFrame = ({ ops, tile, frameId }: MoveTileIntoFrameArgs): boolean => {
  const { tiles, frames, patchFrame, placeTiles } = ops;
  const frame = frames.find((f) => f.id === frameId);
  if (!frame) return false;

  const tilesExcludingThis = tiles.filter((t) => t.id !== tile.id);
  const result = placeTileInFrame({ frame, width: tile.width, height: tile.height, tiles: tilesExcludingThis, frames });

  if (result.frame !== null) {
    patchFrame(frameId, { x: result.frame.x, y: result.frame.y, width: result.frame.width, height: result.frame.height });
  }
  placeTiles([{ id: tile.id, x: result.x, y: result.y }]);

  return result.fitted;
};

interface SpawnInGroupArgs {
  ops: GroupOps;
  group: ProjectGroup;
  width: number;
  height: number;
}

export const spawnInGroup = ({ ops, group, width, height }: SpawnInGroupArgs): { x: number; y: number; fitted: boolean } | null => {
  if (group.kind !== 'frame' || !group.frameId) return null;

  const { tiles, frames, patchFrame } = ops;
  const frame = frames.find((f) => f.id === group.frameId);
  if (!frame) return null;

  const result = placeTileInFrame({ frame, width, height, tiles, frames });

  if (result.frame !== null) {
    patchFrame(group.frameId, { x: result.frame.x, y: result.frame.y, width: result.frame.width, height: result.frame.height });
  }

  return { x: result.x, y: result.y, fitted: result.fitted };
};
