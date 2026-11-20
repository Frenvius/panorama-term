import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { groupByFrame } from '~/usecase/util/frame';
import { snapTermSize } from '~/usecase/util/termGrid';
import { CELL, TILE_MIN_WIDTH, FRAME_BAR_SPACE, FRAME_MIN_WIDTH, TILE_MIN_HEIGHT, FRAME_MIN_HEIGHT } from '~/usecase/util/constants';

export type ReorderMode = 'default' | 'compact';

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ReorderPlan {
  tiles: Map<string, Rect>;
  frames: Map<string, Rect>;
}

interface Box extends Rect {
  id: string;
  lead: number;
}

export interface Spacing {
  gap: number;
  pad: number;
  barSpace: number;
}


export const isReorderMode = (value: unknown): value is ReorderMode => value === 'default' || value === 'compact';

export const layoutSpacing = (mode: ReorderMode, spacedPad: number): Spacing =>
  mode === 'compact' ? { gap: 0, pad: 0, barSpace: 0 } : { gap: spacedPad, pad: spacedPad, barSpace: FRAME_BAR_SPACE };

const isMovable = (tile: Tile): boolean => !tile.pinned && !tile.runCwd;

const toGrid = (value: number): number => Math.round(value / CELL) * CELL;

const ceilGrid = (value: number): number => Math.ceil(value / CELL) * CELL;

const gridTile = (tile: Tile, cellW: number): Box => {
  if (tile.type === 'term') {
    const { width, height } = snapTermSize(tile.width, tile.height, cellW);
    return { id: tile.id, x: tile.x, y: tile.y, lead: 0, width, height };
  }
  return { id: tile.id, x: tile.x, y: tile.y, lead: 0, width: Math.max(TILE_MIN_WIDTH, toGrid(tile.width)), height: Math.max(TILE_MIN_HEIGHT, toGrid(tile.height)) };
};

const readingRows = (boxes: Box[]): Box[][] => {
  const rows: Box[][] = [];
  let rowMid = -Infinity;
  for (const box of [...boxes].sort((a, b) => a.y - b.y || a.x - b.x)) {
    const row = rows[rows.length - 1];
    if (row && box.y < rowMid) {
      row.push(box);
      continue;
    }
    rows.push([box]);
    rowMid = box.y + box.height / 2;
  }
  return rows.map((row) => row.sort((a, b) => a.x - b.x));
};

const packRows = (boxes: Box[], origin: { x: number; y: number }, gap: number): Map<string, Rect> => {
  const placed = new Map<string, Rect>();
  let bottom = origin.y;
  let first = true;
  for (const row of readingRows(boxes)) {
    const lead = Math.max(...row.map((b) => b.lead));
    const y = ceilGrid(bottom + Math.max(first ? 0 : gap, lead));
    let x = origin.x;
    for (const box of row) {
      placed.set(box.id, { x, y, width: box.width, height: box.height });
      x = ceilGrid(x + box.width + gap);
    }
    bottom = y + Math.max(...row.map((b) => b.height));
    first = false;
  }
  return placed;
};

const reorderMembers = (frame: Rect, members: Tile[], spacing: Spacing, cellW: number): Map<string, Rect> =>
  packRows(members.map((t) => gridTile(t, cellW)), { x: frame.x + spacing.pad, y: frame.y + spacing.pad }, spacing.gap);

const frameAroundPlaced = (frame: Rect, placed: Map<string, Rect>, pad: number): Rect => {
  const rects = [...placed.values()];
  const right = Math.max(...rects.map((r) => r.x + r.width));
  const bottom = Math.max(...rects.map((r) => r.y + r.height));
  return {
    x: frame.x,
    y: frame.y,
    width: Math.max(FRAME_MIN_WIDTH, ceilGrid(right + pad - frame.x)),
    height: Math.max(FRAME_MIN_HEIGHT, ceilGrid(bottom + pad - frame.y))
  };
};

const gridFrame = (frame: Frame): Rect => ({
  x: toGrid(frame.x),
  y: toGrid(frame.y),
  width: Math.max(FRAME_MIN_WIDTH, toGrid(frame.width)),
  height: Math.max(FRAME_MIN_HEIGHT, toGrid(frame.height))
});

const planFrame = (frame: Frame, members: Tile[], spacing: Spacing, cellW: number): ReorderPlan => {
  const rect = gridFrame(frame);
  if (!members.length) return { tiles: new Map(), frames: new Map([[frame.id, rect]]) };
  const placed = reorderMembers(rect, members, spacing, cellW);
  return { tiles: placed, frames: new Map([[frame.id, frameAroundPlaced(rect, placed, spacing.pad)]]) };
};

interface PlanFrameReorderArgs {
  id: string;
  cellW: number;
  tiles: Tile[];
  frames: Frame[];
  spacing: Spacing;
}

export const planFrameReorder = ({ id, cellW, tiles, frames, spacing }: PlanFrameReorderArgs): ReorderPlan | null => {
  const frame = frames.find((f) => f.id === id);
  if (!frame) return null;
  const members = (groupByFrame(frames, tiles).members.get(id) ?? []).filter(isMovable);
  if (!members.length) return null;
  return planFrame(frame, members, spacing, cellW);
};

interface PlanCanvasReorderArgs {
  tiles: Tile[];
  frames: Frame[];
  cellW: number;
  spacing: Spacing;
}

export const planCanvasReorder = ({ tiles, frames, cellW, spacing }: PlanCanvasReorderArgs): ReorderPlan | null => {
  const { members, loose } = groupByFrame(frames, tiles);
  const tileRects = new Map<string, Rect>();
  const frameRects = new Map<string, Rect>();
  const memberIds = new Map<string, string[]>();
  const blocks: Box[] = [];

  for (const frame of frames) {
    const plan = planFrame(frame, (members.get(frame.id) ?? []).filter(isMovable), spacing, cellW);
    plan.tiles.forEach((rect, tid) => tileRects.set(tid, rect));
    const rect = plan.frames.get(frame.id)!;
    frameRects.set(frame.id, rect);
    memberIds.set(frame.id, [...plan.tiles.keys()]);
    blocks.push({ id: frame.id, ...rect, lead: spacing.barSpace });
  }
  for (const tile of loose.filter(isMovable)) {
    const box = gridTile(tile, cellW);
    tileRects.set(tile.id, box);
    blocks.push(box);
  }
  if (!blocks.length) return null;

  const origin = { x: toGrid(Math.min(...blocks.map((b) => b.x))), y: toGrid(Math.min(...blocks.map((b) => b.y - b.lead))) };
  const placedBlocks = packRows(blocks, origin, spacing.gap);

  for (const block of blocks) {
    const target = placedBlocks.get(block.id)!;
    const frame = frameRects.get(block.id);
    if (!frame) {
      tileRects.set(block.id, target);
      continue;
    }
    const dx = target.x - frame.x;
    const dy = target.y - frame.y;
    frameRects.set(block.id, { ...frame, x: target.x, y: target.y });
    for (const tid of memberIds.get(block.id) ?? []) {
      const rect = tileRects.get(tid)!;
      tileRects.set(tid, { ...rect, x: rect.x + dx, y: rect.y + dy });
    }
  }
  return { tiles: tileRects, frames: frameRects };
};
