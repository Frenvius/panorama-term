import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { TidyPlan, TidyFrame } from '~/domain/interfaces/orch.interface';
import type { Rect, ReorderPlan } from '~/usecase/util/reorderLayout';

import { frameOf, groupByFrame } from '~/usecase/util/frame';
import { dominantProject, tileOwnerResolver } from '~/usecase/util/projectGroups';
import { currentLayoutSpacing } from '~/usecase/util/reorderSetting';
import { projectName, tileProject, tileCandidatePath, normalizeProjectPath } from '~/usecase/util/projectLayout';
import { CELL, FRAME_COLOR, FRAME_MIN_WIDTH, FRAME_MIN_HEIGHT } from '~/usecase/util/constants';

export interface TidySnapshotOpts {
  projects: string[];
  frames: Frame[];
  tiles: Tile[];
}

const isNote = (tile: Tile): boolean => tile.type === 'note';

const tileTitle = (tile: Tile): string =>
  tile.userTitle ?? tile.oscTitle ?? tile.autoTitle ?? tile.type;

const frameMembers = (frame: Frame, frames: Frame[], tiles: Tile[]): Tile[] =>
  tiles.filter((t) => frameOf(frames, t)?.id === frame.id);

const frameProject = (frame: Frame, members: Tile[], projects: string[]): string | null =>
  frame.path ? normalizeProjectPath(frame.path) || null : dominantProject(members, projects);

const matchProject = (path: string | undefined, projects: string[]): string | null => {
  if (!path) return null;
  const lower = normalizeProjectPath(path).toLowerCase();
  return projects.find((p) => normalizeProjectPath(p).toLowerCase() === lower) ?? null;
};

export const buildTidySnapshot = ({ projects, frames, tiles }: TidySnapshotOpts): string =>
  JSON.stringify({
    projects: projects.map((path, order) => ({ path, name: projectName(path), order })),
    frames: frames.map((f) => ({
      id: f.id,
      title: f.title,
      path: frameProject(f, frameMembers(f, frames, tiles), projects)
    })),
    tiles: tiles.filter((t) => !isNote(t)).map((t) => ({
      id: t.id,
      type: t.type,
      title: tileTitle(t),
      cwd: t.cwd ?? null,
      workspacePath: t.workspacePath ?? null,
      branch: t.branch ?? null,
      project: tileProject(t, projects),
      frameId: frameOf(frames, t)?.id ?? null
    }))
  });

export interface TidyKnown {
  projectPaths: string[];
  tiles: Tile[];
  frames: Frame[];
}

const orderProjects = (planOrder: string[], projectPaths: string[]): string[] => {
  const seenLower = new Set<string>();
  const ordered: string[] = [];
  for (const path of [...planOrder, ...projectPaths]) {
    const canonical = matchProject(path, projectPaths);
    if (!canonical || seenLower.has(canonical.toLowerCase())) continue;
    ordered.push(canonical);
    seenLower.add(canonical.toLowerCase());
  }
  return ordered;
};

interface FrameResolverArgs {
  known: TidyKnown;
  claimed: Set<string>;
}

const frameResolver = ({ known, claimed }: FrameResolverArgs) => {
  const frameById = new Map(known.frames.map((f) => [f.id, f]));
  const projectByFrame = new Map(
    known.frames.map((f) => [f.id, frameProject(f, frameMembers(f, known.frames, known.tiles), known.projectPaths)])
  );
  const frameForProject = (path: string): string | undefined =>
    known.frames.find((f) => !claimed.has(f.id) && projectByFrame.get(f.id)?.toLowerCase() === path.toLowerCase())?.id;

  return (planFrame: TidyFrame, members: Tile[]): { frameId?: string; path?: string } => {
    const requested = planFrame.frameId && frameById.has(planFrame.frameId) && !claimed.has(planFrame.frameId) ? planFrame.frameId : undefined;
    const fixedPath = requested ? frameById.get(requested)!.path : undefined;
    const spansProjects = new Set(members.map((t) => tileProject(t, known.projectPaths)?.toLowerCase()).filter(Boolean)).size > 1;
    const guessedPath = spansProjects ? null : matchProject(planFrame.path, known.projectPaths) || dominantProject(members, known.projectPaths);
    const path = (fixedPath && normalizeProjectPath(fixedPath)) || guessedPath;
    const frameId = requested ?? (path ? frameForProject(path) : undefined);
    if (frameId) claimed.add(frameId);
    return { frameId, path: path ?? undefined };
  };
};

const byProject = (tiles: Tile[], projectPaths: string[]): Tile[] => {
  const rank = new Map<string, number>();
  for (const t of tiles) {
    const key = tileProject(t, projectPaths)?.toLowerCase() ?? '';
    if (!rank.has(key)) rank.set(key, rank.size);
  }
  return [...tiles].sort((a, b) => rank.get(tileProject(a, projectPaths)?.toLowerCase() ?? '')! - rank.get(tileProject(b, projectPaths)?.toLowerCase() ?? '')!);
};

const isInside = (child: string, parent: string): boolean => child.toLowerCase().startsWith(`${parent.toLowerCase()}/`);

const mergeNestedProjects = (frames: TidyFrame[]): TidyFrame[] => {
  const merged = frames.map((f) => ({ ...f, tileIds: [...f.tileIds] }));
  const byDepth = [...merged].sort((a, b) => (b.path?.length ?? 0) - (a.path?.length ?? 0));
  const absorbed = new Set<TidyFrame>();
  for (const child of byDepth) {
    const parent = byDepth
      .filter((f) => f !== child && !absorbed.has(f) && f.path && child.path && isInside(child.path, f.path))
      .reduce<TidyFrame | undefined>((best, f) => (!best || f.path!.length > best.path!.length ? f : best), undefined);
    if (!parent) continue;
    parent.tileIds.push(...child.tileIds);
    absorbed.add(child);
  }
  return merged.filter((f) => !absorbed.has(f));
};

export const sanitizeTidyPlan = (plan: TidyPlan, known: TidyKnown): TidyPlan => {
  const tileById = new Map(known.tiles.filter((t) => !isNote(t)).map((t) => [t.id, t]));
  const assignedTileIds = new Set<string>();
  const lockedFrameIds = new Set(known.frames.filter((f) => f.orderLocked).map((f) => f.id));
  const lockedTileIds = new Set(
    known.frames.filter((f) => f.orderLocked).flatMap((f) => frameMembers(f, known.frames, known.tiles).map((t) => t.id))
  );
  const resolveFrame = frameResolver({ known, claimed: new Set<string>(lockedFrameIds) });
  const frames: TidyFrame[] = [];

  for (const frame of plan.frames) {
    const validTileIds = frame.tileIds.filter((id) => tileById.has(id) && !assignedTileIds.has(id) && !lockedTileIds.has(id));
    if (validTileIds.length === 0) continue;
    for (const id of validTileIds) assignedTileIds.add(id);
    const members = validTileIds.map((id) => tileById.get(id)!);
    const { frameId, path } = resolveFrame(frame, members);
    const tileIds = byProject(members, known.projectPaths).map((t) => t.id);
    frames.push({ title: frame.title, tileIds, ...(frameId && { frameId }), ...(path && { path }) });
  }

  return { projectOrder: orderProjects(plan.projectOrder, known.projectPaths), frames: mergeNestedProjects(frames) };
};

export const pinPathlessNotes = (opts: TidySnapshotOpts): { id: string; workspacePath: string }[] => {
  const ownerOf = tileOwnerResolver(opts);
  return opts.tiles.flatMap((t) => {
    if (!isNote(t) || tileCandidatePath(t)) return [];
    const project = ownerOf(t);
    return project ? [{ id: t.id, workspacePath: project }] : [];
  });
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

const completeFrame = (frame: Frame, path: string | null): Partial<Omit<Frame, 'id'>> => {
  const patch: Partial<Omit<Frame, 'id'>> = {};
  if (!frame.path && path) patch.path = path;
  if (!frame.title.trim() && path) patch.title = projectName(path);
  if (!frame.color) patch.color = FRAME_COLOR;
  return patch;
};

export const computeTidyLayout = (plan: TidyPlan, tiles: Tile[], frames: Frame[], projects: string[] = []): TidyLayoutResult => {
  const { pad, gap } = currentLayoutSpacing();
  const tileMap = new Map(tiles.map((t) => [t.id, t]));
  const frameMap = new Map(frames.map((f) => [f.id, f]));
  const plannedTileIds = new Set(plan.frames.flatMap((f) => f.tileIds));
  const handledFrameIds = new Set<string>();

  const tileMoves: { id: string; x: number; y: number }[] = [];
  const framePatches: { id: string; patch: Partial<Omit<Frame, 'id'>> }[] = [];
  const newFrameList: Omit<Frame, 'id'>[] = [];

  let rightEdge = frames.reduce(
    (m, f) => Math.max(m, f.x + f.width),
    tiles.reduce((m, t) => Math.max(m, t.x + t.width), 0)
  );
  const tileTopY = tiles.length > 0 ? tiles.reduce((m, t) => Math.min(m, t.y), Infinity) : 0;
  const safeTopY = isFinite(tileTopY) ? tileTopY : 0;

  const placeRight = (frameTiles: Tile[]) => {
    const localMoves = layoutGrid(frameTiles, rightEdge + gap * 2 + pad, safeTopY + pad, gap);
    const frameBox = frameBoxFromMoves(localMoves, pad);
    tileMoves.push(...localMoves.map((m) => ({ id: m.id, x: m.x, y: m.y })));
    rightEdge = frameBox.x + frameBox.width;
    return frameBox;
  };

  for (const pf of plan.frames) {
    const plannedTiles = pf.tileIds.map((id) => tileMap.get(id)).filter((t): t is Tile => !!t);
    if (!plannedTiles.length) continue;

    const existingFrame = pf.frameId ? frameMap.get(pf.frameId) : undefined;
    const title = pf.title.trim() || (pf.path ? projectName(pf.path) : 'Frame');

    if (!existingFrame) {
      newFrameList.push({ ...placeRight(plannedTiles), title, color: FRAME_COLOR, ...(pf.path && { path: pf.path }) });
      continue;
    }

    handledFrameIds.add(existingFrame.id);
    const keptTiles = frameMembers(existingFrame, frames, tiles).filter((t) => !plannedTileIds.has(t.id));
    const frameTiles = [...plannedTiles, ...keptTiles];
    const memberIds = new Set(frameTiles.map((t) => t.id));
    const localMoves = layoutGrid(frameTiles, existingFrame.x + pad, existingFrame.y + pad, gap);
    const inPlaceBox = frameBoxFromMoves(localMoves, pad);

    const collides =
      frames.some(
        (f) =>
          f.id !== existingFrame.id &&
          rectsIntersect(inPlaceBox.x, inPlaceBox.y, inPlaceBox.width, inPlaceBox.height, f.x, f.y, f.width, f.height)
      ) ||
      tiles.some(
        (t) =>
          !memberIds.has(t.id) &&
          rectsIntersect(inPlaceBox.x, inPlaceBox.y, inPlaceBox.width, inPlaceBox.height, t.x, t.y, t.width, t.height)
      );

    let frameBox = inPlaceBox;
    if (collides) {
      frameBox = placeRight(frameTiles);
    } else {
      tileMoves.push(...localMoves.map((m) => ({ id: m.id, x: m.x, y: m.y })));
    }
    framePatches.push({
      id: existingFrame.id,
      patch: { ...completeFrame(existingFrame, pf.path ?? null), ...frameBox, title }
    });
  }

  for (const frame of frames) {
    if (handledFrameIds.has(frame.id)) continue;
    const patch = completeFrame(frame, frameProject(frame, frameMembers(frame, frames, tiles), projects));
    if (Object.keys(patch).length > 0) framePatches.push({ id: frame.id, patch });
  }

  return { tileMoves, framePatches, newFrames: newFrameList };
};

export type ClusterPlanner = (tiles: Tile[], frames: Frame[]) => ReorderPlan | null;

export interface SettledTidyLayout {
  tileRects: Map<string, Rect>;
  framePatches: { id: string; patch: Partial<Omit<Frame, 'id'>> }[];
  newFrames: Omit<Frame, 'id'>[];
  removedFrameIds: string[];
}

const NEW_FRAME_PREFIX = 'tidy-new-';

const sameRect = (tile: Tile, rect: Rect): boolean =>
  tile.x === rect.x && tile.y === rect.y && tile.width === rect.width && tile.height === rect.height;

const centerOf = (rects: Rect[]): { x: number; y: number } => {
  const left = Math.min(...rects.map((r) => r.x));
  const top = Math.min(...rects.map((r) => r.y));
  const right = Math.max(...rects.map((r) => r.x + r.width));
  const bottom = Math.max(...rects.map((r) => r.y + r.height));
  return { x: (left + right) / 2, y: (top + bottom) / 2 };
};

const recenter = (plan: ReorderPlan | null | undefined, target: { x: number; y: number }): ReorderPlan | null => {
  if (!plan) return null;
  const current = centerOf([...plan.tiles.values(), ...plan.frames.values()]);
  const dx = Math.round((target.x - current.x) / CELL) * CELL;
  const dy = Math.round((target.y - current.y) / CELL) * CELL;
  const shift = (rects: Map<string, Rect>) => new Map([...rects].map(([id, r]) => [id, { ...r, x: r.x + dx, y: r.y + dy }]));
  return { tiles: shift(plan.tiles), frames: shift(plan.frames) };
};

export const settleTidyLayout = (layout: TidyLayoutResult, tiles: Tile[], frames: Frame[], cluster?: ClusterPlanner): SettledTidyLayout => {
  const moves = new Map(layout.tileMoves.map((m) => [m.id, m]));
  const patches = new Map(layout.framePatches.map((p) => [p.id, p.patch]));
  const movedTiles = tiles.map((t) => ({ ...t, ...moves.get(t.id) }));
  const created = layout.newFrames.map((f, i) => ({ ...f, id: `${NEW_FRAME_PREFIX}${i}` }));
  const nextFrames = [...frames.map((f) => ({ ...f, ...patches.get(f.id) })), ...created];

  const before = groupByFrame(frames, tiles).members;
  const after = groupByFrame(nextFrames, movedTiles).members;
  const removed = new Set(frames.filter((f) => before.get(f.id)?.length && !after.get(f.id)?.length).map((f) => f.id));
  const keptFrames = nextFrames.filter((f) => !removed.has(f.id));
  const packed = recenter(cluster?.(movedTiles, keptFrames), centerOf([...tiles, ...frames]));

  const tileRects = new Map<string, Rect>();
  for (const [i, tile] of movedTiles.entries()) {
    const rect = packed?.tiles.get(tile.id) ?? tile;
    const target = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    if (!sameRect(tiles[i]!, target)) tileRects.set(tile.id, target);
  }

  const framePatches = frames.flatMap((f) => {
    if (removed.has(f.id)) return [];
    const patch = { ...patches.get(f.id), ...packed?.frames.get(f.id) };
    return Object.keys(patch).length ? [{ id: f.id, patch }] : [];
  });
  const newFrames = created.map(({ id, ...frame }) => ({ ...frame, ...packed?.frames.get(id) }));

  return { tileRects, framePatches, newFrames, removedFrameIds: [...removed] };
};
