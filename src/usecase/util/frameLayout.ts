import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { frameOf } from '~/usecase/util/frame';

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const FRAME_PADDING = 24;
const RING_STEP = 40;
const MAX_RINGS = 200;
const GATHER_GAP = 24;
const MAX_ROW_WIDTH = 2400;
const SCAN_STEP = 24;

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

export const frameAroundTiles = (tiles: Tile[]): Rect => {
  if (tiles.length === 0) return { x: 0, y: 0, width: 0, height: 0 };
  const minX = Math.min(...tiles.map((t) => t.x)) - FRAME_PADDING;
  const minY = Math.min(...tiles.map((t) => t.y)) - FRAME_PADDING;
  const maxX = Math.max(...tiles.map((t) => t.x + t.width)) + FRAME_PADDING;
  const maxY = Math.max(...tiles.map((t) => t.y + t.height)) + FRAME_PADDING;
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
  groupTiles: Tile[];
}

export const planGroupFrame = ({
  groupTiles,
  allTiles,
  frames
}: PlanGroupFrameArgs): { kind: 'wrap'; rect: Rect } | { kind: 'gather'; rect: Rect; moves: { id: string; x: number; y: number }[] } => {
  const bbox = frameAroundTiles(groupTiles);
  const groupIds = new Set(groupTiles.map((t) => t.id));
  const outsideTiles = allTiles.filter((t) => !groupIds.has(t.id));
  const hasOutsideInBbox = outsideTiles.some((t) => centerInRect(t, bbox));

  if (!hasOutsideInBbox) return { kind: 'wrap', rect: bbox };

  const rowContentMax = MAX_ROW_WIDTH - 2 * FRAME_PADDING;
  const relPos: { id: string; rx: number; ry: number }[] = [];
  let rowX = FRAME_PADDING;
  let rowY = FRAME_PADDING;
  let rowMaxH = 0;

  for (const tile of groupTiles) {
    if (rowX > FRAME_PADDING && rowX + tile.width - FRAME_PADDING > rowContentMax) {
      rowY += rowMaxH + GATHER_GAP;
      rowX = FRAME_PADDING;
      rowMaxH = 0;
    }
    relPos.push({ id: tile.id, rx: rowX, ry: rowY });
    rowX += tile.width + GATHER_GAP;
    rowMaxH = Math.max(rowMaxH, tile.height);
  }

  let frameW = 0;
  for (let i = 0; i < relPos.length; i++) {
    const p = relPos[i]!;
    const tile = groupTiles[i]!;
    frameW = Math.max(frameW, p.rx + tile.width + FRAME_PADDING);
  }
  const frameH = rowY + rowMaxH + FRAME_PADDING;

  const nearTile = groupTiles[0]!;
  const obstacles: Rect[] = [
    ...outsideTiles.map((t) => ({ x: t.x, y: t.y, width: t.width, height: t.height })),
    ...frames.map((f) => ({ x: f.x, y: f.y, width: f.width, height: f.height }))
  ];

  const { x: fx, y: fy } = findFreeRect({ width: frameW, height: frameH, near: { x: nearTile.x, y: nearTile.y }, obstacles });
  const rect: Rect = { x: fx, y: fy, width: frameW, height: frameH };

  const moves = relPos.map((p) => ({ id: p.id, x: fx + p.rx, y: fy + p.ry }));

  return { kind: 'gather', rect, moves };
};

interface PlaceTileInFrameArgs {
  frame: Frame;
  width: number;
  height: number;
  tiles: Tile[];
  frames: Frame[];
}

export const placeTileInFrame = ({
  frame,
  width,
  height,
  tiles,
  frames
}: PlaceTileInFrameArgs): { x: number; y: number; frame: Rect | null; fitted: boolean } => {
  const memberTiles = tiles.filter((t) => frameOf(frames, t)?.id === frame.id);
  const otherFrames = frames.filter((f) => f.id !== frame.id);

  const interiorX0 = frame.x + FRAME_PADDING;
  const interiorY0 = frame.y + FRAME_PADDING;
  const interiorX1 = frame.x + frame.width - FRAME_PADDING - width;
  const interiorY1 = frame.y + frame.height - FRAME_PADDING - height;

  if (interiorX1 >= interiorX0 && interiorY1 >= interiorY0) {
    for (let sy = interiorY0; sy <= interiorY1; sy += SCAN_STEP) {
      for (let sx = interiorX0; sx <= interiorX1; sx += SCAN_STEP) {
        const overlaps = memberTiles.some((t) => rectsOverlap({ x: sx, y: sy, width, height }, t));
        if (!overlaps) return { x: sx, y: sy, frame: null, fitted: true };
      }
    }
  }

  const nonMemberTiles = tiles.filter((t) => frameOf(frames, t)?.id !== frame.id);

  const contentRight =
    memberTiles.length > 0 ? Math.max(...memberTiles.map((t) => t.x + t.width)) : frame.x + FRAME_PADDING;
  const tileX = contentRight + GATHER_GAP;
  const tileY = frame.y + FRAME_PADDING;
  const fitsVertically = tileY + height <= frame.y + frame.height - FRAME_PADDING;
  const newFrameWidth = tileX + width + FRAME_PADDING - frame.x;

  if (fitsVertically && newFrameWidth > frame.width) {
    const addedRect: Rect = { x: frame.x + frame.width, y: frame.y, width: newFrameWidth - frame.width, height: frame.height };
    const grownFrame: Rect = { x: frame.x, y: frame.y, width: newFrameWidth, height: frame.height };
    const noOutsideCenters = nonMemberTiles.every((t) => !centerInRect(t, addedRect));
    const noFrameConflict = otherFrames.every((f) => !rectsOverlap(addedRect, f));
    if (noOutsideCenters && noFrameConflict) {
      return { x: tileX, y: tileY, frame: grownFrame, fitted: true };
    }
  }

  const contentBottom =
    memberTiles.length > 0 ? Math.max(...memberTiles.map((t) => t.y + t.height)) : frame.y + FRAME_PADDING;
  const tileY3 = contentBottom + GATHER_GAP;
  const tileX3 = frame.x + FRAME_PADDING;
  const fitsHorizontally = tileX3 + width <= frame.x + frame.width - FRAME_PADDING;
  const newFrameHeight = tileY3 + height + FRAME_PADDING - frame.y;

  if (fitsHorizontally && newFrameHeight > frame.height) {
    const addedRect: Rect = { x: frame.x, y: frame.y + frame.height, width: frame.width, height: newFrameHeight - frame.height };
    const grownFrame: Rect = { x: frame.x, y: frame.y, width: frame.width, height: newFrameHeight };
    const noOutsideCenters = nonMemberTiles.every((t) => !centerInRect(t, addedRect));
    const noFrameConflict = otherFrames.every((f) => !rectsOverlap(addedRect, f));
    if (noOutsideCenters && noFrameConflict) {
      return { x: tileX3, y: tileY3, frame: grownFrame, fitted: true };
    }
  }

  const allObstacles: Rect[] = [
    ...tiles.map((t) => ({ x: t.x, y: t.y, width: t.width, height: t.height })),
    ...frames.map((f) => ({ x: f.x, y: f.y, width: f.width, height: f.height }))
  ];
  const { x: fx, y: fy } = findFreeRect({
    width,
    height,
    near: { x: frame.x + frame.width + GATHER_GAP, y: frame.y },
    obstacles: allObstacles
  });
  return { x: fx, y: fy, frame: null, fitted: false };
};
