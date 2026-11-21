import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { TidyPlan, TidyFrame } from '~/domain/interfaces/orch.interface';

import { tileInFrame } from '~/usecase/util/frame';
import { projectName } from '~/usecase/util/projectLayout';
import { currentLayoutSpacing } from '~/usecase/util/reorderSetting';
import { CELL, FRAME_COLOR, FRAME_MIN_WIDTH, FRAME_MIN_HEIGHT } from '~/usecase/util/constants';

export interface TidySnapshotOpts {
  projects: string[];
  frames: Frame[];
  tiles: Tile[];
}

const tileTitle = (tile: Tile): string =>
  tile.userTitle ?? tile.oscTitle ?? tile.autoTitle ?? tile.type;

export const buildTidySnapshot = ({ projects, frames, tiles }: TidySnapshotOpts): string => {
  const tileFrameId = new Map<string, string>();
  for (const frame of frames) {
    for (const tile of tiles) {
      if (tileInFrame(frame, tile)) tileFrameId.set(tile.id, frame.id);
    }
  }

  return JSON.stringify({
    projects: projects.map((path, order) => ({ path, name: projectName(path), order })),
    frames: frames.map((f) => ({ id: f.id, title: f.title, path: f.path ?? null })),
    tiles: tiles.map((t) => ({
      id: t.id,
      type: t.type,
      title: tileTitle(t),
      cwd: t.cwd ?? null,
      workspacePath: t.workspacePath ?? null,
      branch: t.branch ?? null,
      frameId: tileFrameId.get(t.id) ?? null
    }))
  });
};

export interface TidyKnown {
  projectPaths: string[];
  tileIds: string[];
}

export const sanitizeTidyPlan = (plan: TidyPlan, known: TidyKnown): TidyPlan => {
  const knownPathLower = new Set(known.projectPaths.map((p) => p.toLowerCase()));
  const knownTileIds = new Set(known.tileIds);

  const seenLower = new Set<string>();
  const orderedProjects: string[] = [];

  for (const path of plan.projectOrder) {
    const lower = path.toLowerCase();
    if (knownPathLower.has(lower) && !seenLower.has(lower)) {
      const canonical = known.projectPaths.find((p) => p.toLowerCase() === lower)!;
      orderedProjects.push(canonical);
      seenLower.add(lower);
    }
  }
  for (const path of known.projectPaths) {
    const lower = path.toLowerCase();
    if (!seenLower.has(lower)) {
      orderedProjects.push(path);
      seenLower.add(lower);
    }
  }

  const assignedTileIds = new Set<string>();
  const frames: TidyFrame[] = [];

  for (const frame of plan.frames) {
    const validTileIds = frame.tileIds.filter((id) => knownTileIds.has(id) && !assignedTileIds.has(id));
    for (const id of validTileIds) assignedTileIds.add(id);
    if (validTileIds.length > 0) {
      frames.push({ ...frame, tileIds: validTileIds });
    }
  }

  return { projectOrder: orderedProjects, frames };
};

const GRID_COLS = 3;

type LocalMove = { id: string; x: number; y: number; width: number; height: number };

const layoutGrid = (frameTiles: Tile[], anchorX: number, anchorY: number, gap: number): LocalMove[] => {
  const moves: LocalMove[] = [];
  let col = 0;
  let rowX = anchorX;
  let rowY = anchorY;
  let rowMaxH = 0;
  for (const tile of frameTiles) {
    moves.push({ id: tile.id, x: rowX, y: rowY, width: tile.width, height: tile.height });
    rowMaxH = Math.max(rowMaxH, tile.height);
    rowX += tile.width + gap;
    col++;
    if (col >= GRID_COLS) {
      col = 0;
      rowX = anchorX;
      rowY += rowMaxH + gap;
      rowMaxH = 0;
    }
  }
  return moves;
};

const frameBoxFromMoves = (moves: LocalMove[], pad: number): { x: number; y: number; width: number; height: number } => {
  const minX = Math.min(...moves.map((m) => m.x)) - pad;
  const minY = Math.min(...moves.map((m) => m.y)) - pad;
  const maxX = Math.max(...moves.map((m) => m.x + m.width)) + pad;
  const maxY = Math.max(...moves.map((m) => m.y + m.height)) + pad;
  return {
    x: Math.round(minX / CELL) * CELL,
    y: Math.round(minY / CELL) * CELL,
    width: Math.max(FRAME_MIN_WIDTH, Math.round((maxX - minX) / CELL) * CELL),
    height: Math.max(FRAME_MIN_HEIGHT, Math.round((maxY - minY) / CELL) * CELL)
  };
};

const rectsIntersect = (ax: number, ay: number, aw: number, ah: number, bx: number, by: number, bw: number, bh: number): boolean =>
  ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by;

export interface TidyLayoutResult {
  tileMoves: { id: string; x: number; y: number }[];
  framePatches: { id: string; patch: Partial<Omit<Frame, 'id'>> }[];
  newFrames: Omit<Frame, 'id'>[];
}

export const computeTidyLayout = (plan: TidyPlan, tiles: Tile[], frames: Frame[]): TidyLayoutResult => {
  const { pad, gap } = currentLayoutSpacing();
  const tileMap = new Map(tiles.map((t) => [t.id, t]));
  const frameMap = new Map(frames.map((f) => [f.id, f]));

  const tileMoves: { id: string; x: number; y: number }[] = [];
  const framePatches: { id: string; patch: Partial<Omit<Frame, 'id'>> }[] = [];
  const newFrameList: Omit<Frame, 'id'>[] = [];

  let rightEdge = frames.reduce(
    (m, f) => Math.max(m, f.x + f.width),
    tiles.reduce((m, t) => Math.max(m, t.x + t.width), 0)
  );
  const tileTopY = tiles.length > 0 ? tiles.reduce((m, t) => Math.min(m, t.y), Infinity) : 0;
  const safeTopY = isFinite(tileTopY) ? tileTopY : 0;

  for (const pf of plan.frames) {
    const frameTiles = pf.tileIds.map((id) => tileMap.get(id)).filter((t): t is Tile => !!t);
    if (!frameTiles.length) continue;

    const memberIds = new Set(pf.tileIds);
    const existingFrame = pf.frameId ? frameMap.get(pf.frameId) : undefined;

    if (existingFrame) {
      const localMoves = layoutGrid(frameTiles, existingFrame.x + pad, existingFrame.y + pad, gap);
      const frameBox = frameBoxFromMoves(localMoves, pad);

      const collides =
        frames.some(
          (f) =>
            f.id !== existingFrame.id &&
            rectsIntersect(frameBox.x, frameBox.y, frameBox.width, frameBox.height, f.x, f.y, f.width, f.height)
        ) ||
        tiles.some(
          (t) =>
            !memberIds.has(t.id) &&
            rectsIntersect(frameBox.x, frameBox.y, frameBox.width, frameBox.height, t.x, t.y, t.width, t.height)
        );

      if (!collides) {
        tileMoves.push(...localMoves.map((m) => ({ id: m.id, x: m.x, y: m.y })));
        framePatches.push({ id: existingFrame.id, patch: { ...frameBox, title: pf.title } });
        continue;
      }
    }

    const localMoves = layoutGrid(frameTiles, rightEdge + gap * 2 + pad, safeTopY + pad, gap);
    const frameBox = frameBoxFromMoves(localMoves, pad);
    tileMoves.push(...localMoves.map((m) => ({ id: m.id, x: m.x, y: m.y })));
    newFrameList.push({ ...frameBox, title: pf.title, color: FRAME_COLOR });
    rightEdge = frameBox.x + frameBox.width;
  }

  return { tileMoves, framePatches, newFrames: newFrameList };
};
