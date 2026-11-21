import type { Spacing } from '~/usecase/util/reorderLayout';
import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { frameOf } from '~/usecase/util/frame';

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const RING_STEP = 40;
const MAX_RINGS = 200;
const MAX_ROW_WIDTH = 2400;

const ringCoords = (ring: number): [number, number][] => {
  if (ring === 0) return [[0, 0]];
  const coords: [number, number][] = [];
  for (let col = -ring; col <= ring; col++) coords.push([col, -ring]);
  for (let row = -ring + 1; row <= ring; row++) coords.push([ring, row]);
  for (let col = ring - 1; col >= -ring; col--) coords.push([col, ring]);
  for (let row = ring - 1; row >= -ring + 1; row--) coords.push([-ring, row]);
  return coords;
};

const rectsOverlap = (a: Rect, b: Rect): boolean =>
  a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;

const centerInRect = (tile: Tile, rect: Rect): boolean => {
  const cx = tile.x + tile.width / 2;
  const cy = tile.y + tile.height / 2;
  return cx >= rect.x && cx < rect.x + rect.width && cy >= rect.y && cy < rect.y + rect.height;
};

export const frameAroundTiles = (tiles: Tile[], pad: number): Rect => {
  if (tiles.length === 0) return { x: 0, y: 0, width: 0, height: 0 };
  const minX = Math.min(...tiles.map((t) => t.x)) - pad;
  const minY = Math.min(...tiles.map((t) => t.y)) - pad;
  const maxX = Math.max(...tiles.map((t) => t.x + t.width)) + pad;
  const maxY = Math.max(...tiles.map((t) => t.y + t.height)) + pad;
  return {
    x: Math.round(minX),
    y: Math.round(minY),
    width: Math.round(maxX - minX),
    height: Math.round(maxY - minY)
  };
};

interface FindFreeRectArgs {
  width: number;
  height: number;
  near: { x: number; y: number };
  obstacles: Rect[];
  gap?: number;
}

export const findFreeRect = ({ width, height, near, obstacles, gap = 16 }: FindFreeRectArgs): { x: number; y: number } => {
  const blocked = (x: number, y: number): boolean => {
    for (const ob of obstacles) {
      if (rectsOverlap({ x: x - gap, y: y - gap, width: width + 2 * gap, height: height + 2 * gap }, ob)) return true;
    }
    return false;
  };

  for (let ring = 0; ring <= MAX_RINGS; ring++) {
    for (const [dcol, drow] of ringCoords(ring)) {
      const x = near.x + dcol * RING_STEP;
      const y = near.y + drow * RING_STEP;
      if (!blocked(x, y)) return { x, y };
    }
  }

  const maxRight = obstacles.length > 0 ? Math.max(...obstacles.map((o) => o.x + o.width)) : near.x;
  return { x: maxRight + gap, y: near.y };
};

interface PlanGroupFrameArgs {
  frames: Frame[];
  allTiles: Tile[];
  spacing: Spacing;
  groupTiles: Tile[];
}

export const planGroupFrame = ({
  groupTiles,
  allTiles,
  frames,
  spacing
}: PlanGroupFrameArgs): { kind: 'wrap'; rect: Rect } | { kind: 'gather'; rect: Rect; moves: { id: string; x: number; y: number }[] } => {
  const { gap, pad } = spacing;
  const bbox = frameAroundTiles(groupTiles, pad);
  const groupIds = new Set(groupTiles.map((t) => t.id));
  const outsideTiles = allTiles.filter((t) => !groupIds.has(t.id));
  const hasOutsideInBbox = outsideTiles.some((t) => centerInRect(t, bbox));

  if (!hasOutsideInBbox) return { kind: 'wrap', rect: bbox };

  const rowContentMax = MAX_ROW_WIDTH - 2 * pad;
  const relPos: { id: string; rx: number; ry: number }[] = [];
  let rowX = pad;
  let rowY = pad;
  let rowMaxH = 0;

  for (const tile of groupTiles) {
    if (rowX > pad && rowX + tile.width - pad > rowContentMax) {
      rowY += rowMaxH + gap;
      rowX = pad;
      rowMaxH = 0;
    }
    relPos.push({ id: tile.id, rx: rowX, ry: rowY });
    rowX += tile.width + gap;
    rowMaxH = Math.max(rowMaxH, tile.height);
  }

  let frameW = 0;
  for (let i = 0; i < relPos.length; i++) {
    const p = relPos[i]!;
    const tile = groupTiles[i]!;
    frameW = Math.max(frameW, p.rx + tile.width + pad);
  }
  const frameH = rowY + rowMaxH + pad;

  const nearTile = groupTiles[0]!;
  const obstacles: Rect[] = [
    ...outsideTiles.map((t) => ({ x: t.x, y: t.y, width: t.width, height: t.height })),
    ...frames.map((f) => ({ x: f.x, y: f.y, width: f.width, height: f.height }))
  ];

  const { x: fx, y: fy } = findFreeRect({
    gap,
    obstacles,
    width: frameW,
    height: frameH,
    near: { x: nearTile.x, y: nearTile.y }
  });
  const rect: Rect = { x: fx, y: fy, width: frameW, height: frameH };

  const moves = relPos.map((p) => ({ id: p.id, x: fx + p.rx, y: fy + p.ry }));

  return { kind: 'gather', rect, moves };
};

interface PlaceTileInFrameArgs {
  frame: Frame;
  width: number;
  tiles: Tile[];
  height: number;
  frames: Frame[];
  spacing: Spacing;
}

const edgeCandidates = (start: number, end: number, edges: number[]): number[] =>
  [...new Set([start, ...edges])].filter((v) => v >= start && v <= end).sort((a, b) => a - b);

export const placeTileInFrame = ({
  frame,
  width,
  height,
  tiles,
  frames,
  spacing
}: PlaceTileInFrameArgs): { x: number; y: number; frame: Rect | null; fitted: boolean } => {
  const { gap, pad } = spacing;
  const memberTiles = tiles.filter((t) => frameOf(frames, t)?.id === frame.id);
  const otherFrames = frames.filter((f) => f.id !== frame.id);

  const xs = edgeCandidates(
    frame.x + pad,
    frame.x + frame.width - pad - width,
    memberTiles.map((t) => t.x + t.width + gap)
  );
  const ys = edgeCandidates(
    frame.y + pad,
    frame.y + frame.height - pad - height,
    memberTiles.map((t) => t.y + t.height + gap)
  );
  for (const sy of ys) {
    for (const sx of xs) {
      const halo = { x: sx - gap, y: sy - gap, width: width + 2 * gap, height: height + 2 * gap };
      if (!memberTiles.some((t) => rectsOverlap(halo, t))) return { x: sx, y: sy, frame: null, fitted: true };
    }
  }

  const nonMemberTiles = tiles.filter((t) => frameOf(frames, t)?.id !== frame.id);

  const tileX = memberTiles.length > 0 ? Math.max(...memberTiles.map((t) => t.x + t.width)) + gap : frame.x + pad;
  const tileY = frame.y + pad;
  const fitsVertically = tileY + height <= frame.y + frame.height - pad;
  const newFrameWidth = tileX + width + pad - frame.x;

  if (fitsVertically && newFrameWidth > frame.width) {
    const addedRect: Rect = { x: frame.x + frame.width, y: frame.y, width: newFrameWidth - frame.width, height: frame.height };
    const grownFrame: Rect = { x: frame.x, y: frame.y, width: newFrameWidth, height: frame.height };
    const noOutsideCenters = nonMemberTiles.every((t) => !centerInRect(t, addedRect));
    const noFrameConflict = otherFrames.every((f) => !rectsOverlap(addedRect, f));
    if (noOutsideCenters && noFrameConflict) {
      return { x: tileX, y: tileY, frame: grownFrame, fitted: true };
    }
  }

  const tileY3 = memberTiles.length > 0 ? Math.max(...memberTiles.map((t) => t.y + t.height)) + gap : frame.y + pad;
  const tileX3 = frame.x + pad;
  const fitsHorizontally = tileX3 + width <= frame.x + frame.width - pad;
  const newFrameHeight = tileY3 + height + pad - frame.y;

  if (fitsHorizontally && newFrameHeight > frame.height) {
    const addedRect: Rect = { x: frame.x, y: frame.y + frame.height, width: frame.width, height: newFrameHeight - frame.height };
    const grownFrame: Rect = { x: frame.x, y: frame.y, width: frame.width, height: newFrameHeight };
    const noOutsideCenters = nonMemberTiles.every((t) => !centerInRect(t, addedRect));
    const noFrameConflict = otherFrames.every((f) => !rectsOverlap(addedRect, f));
    if (noOutsideCenters && noFrameConflict) {
      return { x: tileX3, y: tileY3, frame: grownFrame, fitted: true };
    }
  }

  const grownBoth: Rect = {
    x: frame.x,
    y: frame.y,
    width: Math.max(frame.width, tileX + width + pad - frame.x),
    height: Math.max(frame.height, tileY + height + pad - frame.y)
  };
  const bothFree =
    nonMemberTiles.every((t) => !centerInRect(t, grownBoth)) && otherFrames.every((f) => !rectsOverlap(grownBoth, f));
  if (bothFree) return { x: tileX, y: tileY, frame: grownBoth, fitted: true };

  const allObstacles: Rect[] = [
    ...tiles.map((t) => ({ x: t.x, y: t.y, width: t.width, height: t.height })),
    ...frames.map((f) => ({ x: f.x, y: f.y, width: f.width, height: f.height }))
  ];
  const { x: fx, y: fy } = findFreeRect({
    gap,
    width,
    height,
    obstacles: allObstacles,
    near: { x: frame.x + frame.width + gap, y: frame.y }
  });
  return { x: fx, y: fy, frame: null, fitted: false };
};

interface PlaceBesideArgs {
  src: Tile;
  tiles: Tile[];
  frames: Frame[];
  spacing: Spacing;
}

export const placeBeside = ({
  src,
  tiles,
  frames,
  spacing
}: PlaceBesideArgs): { x: number; y: number; frame: { id: string; rect: Rect } | null } => {
  const frame = frameOf(frames, src);
  if (frame) {
    const placed = placeTileInFrame({ frame, tiles, frames, spacing, width: src.width, height: src.height });
    return { x: placed.x, y: placed.y, frame: placed.frame ? { id: frame.id, rect: placed.frame } : null };
  }
  const { x, y } = findFreeRect({
    gap: spacing.gap,
    width: src.width,
    height: src.height,
    obstacles: [...tiles, ...frames],
    near: { x: src.x + src.width + spacing.gap, y: src.y }
  });
  return { x, y, frame: null };
};
