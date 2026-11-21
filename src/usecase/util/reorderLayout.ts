import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { groupByFrame } from '~/usecase/util/frame';
import { snapTermSize } from '~/usecase/util/termGrid';
import { CELL, TILE_MIN_WIDTH, FRAME_BAR_SPACE, FRAME_MIN_WIDTH, TILE_MIN_HEIGHT, FRAME_MIN_HEIGHT } from '~/usecase/util/constants';

export type ReorderMode = 'default' | 'compact';

export type FrameArrangement = 'auto' | 'horizontal' | 'vertical' | 'grid2' | 'grid3';

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

const isTight = (spacing: Spacing): boolean => spacing.gap === 0;

const edgeOf = (spacing: Spacing): ((value: number) => number) => (isTight(spacing) ? (value) => value : ceilGrid);

const packRows = (boxes: Box[], origin: { x: number; y: number }, spacing: Spacing): Map<string, Rect> => {
  const placed = new Map<string, Rect>();
  const edge = edgeOf(spacing);
  let bottom = origin.y;
  let first = true;
  for (const row of readingRows(boxes)) {
    const lead = Math.max(...row.map((b) => b.lead));
    const y = edge(bottom + Math.max(first ? 0 : spacing.gap, lead));
    let x = origin.x;
    for (const box of row) {
      placed.set(box.id, { x, y, width: box.width, height: box.height });
      x = edge(x + box.width + spacing.gap);
    }
    bottom = y + Math.max(...row.map((b) => b.height));
    first = false;
  }
  return placed;
};

const GRID_COLUMNS: Record<Exclude<FrameArrangement, 'auto'>, (count: number) => number> = {
  horizontal: (count) => count,
  vertical: () => 1,
  grid2: () => 2,
  grid3: () => 3
};

const arrangeGrid = (origin: { x: number; y: number }, boxes: Box[], columns: number, spacing: Spacing): Map<string, Rect> => {
  const order = readingRows(boxes).flat();
  const edge = edgeOf(spacing);
  const colWidths = Array.from({ length: Math.min(columns, order.length) }, (_, c) =>
    Math.max(...order.filter((_, i) => i % columns === c).map((b) => b.width))
  );
  const rowHeights = Array.from({ length: Math.ceil(order.length / columns) }, (_, r) =>
    Math.max(...order.slice(r * columns, (r + 1) * columns).map((b) => b.height))
  );
  const starts = (sizes: number[], from: number): number[] =>
    sizes.reduce<number[]>((acc, _, i) => [...acc, i ? edge(acc[i - 1]! + sizes[i - 1]! + spacing.gap) : from], []);
  const xs = starts(colWidths, origin.x);
  const ys = starts(rowHeights, origin.y);
  const placed = new Map<string, Rect>();
  order.forEach((box, i) => placed.set(box.id, { x: xs[i % columns]!, y: ys[Math.floor(i / columns)]!, width: box.width, height: box.height }));
  return placed;
};

const frameAroundPlaced = (frame: Rect, placed: Map<string, Rect>, spacing: Spacing): Rect => {
  const rects = [...placed.values()];
  const edge = edgeOf(spacing);
  const right = Math.max(...rects.map((r) => r.x + r.width));
  const bottom = Math.max(...rects.map((r) => r.y + r.height));
  return {
    x: frame.x,
    y: frame.y,
    width: Math.max(FRAME_MIN_WIDTH, edge(right + spacing.pad - frame.x)),
    height: Math.max(FRAME_MIN_HEIGHT, edge(bottom + spacing.pad - frame.y))
  };
};

const gridFrame = (frame: Frame): Rect => ({
  x: toGrid(frame.x),
  y: toGrid(frame.y),
  width: Math.max(FRAME_MIN_WIDTH, toGrid(frame.width)),
  height: Math.max(FRAME_MIN_HEIGHT, toGrid(frame.height))
});

interface PlanFrameArgs {
  frame: Frame;
  members: Tile[];
  spacing: Spacing;
  cellW: number;
  arrangement: FrameArrangement;
}

const planFrame = ({ frame, members, spacing, cellW, arrangement }: PlanFrameArgs): ReorderPlan => {
  const rect = gridFrame(frame);
  if (!members.length) return { tiles: new Map(), frames: new Map([[frame.id, rect]]) };
  const boxes = members.map((t) => gridTile(t, cellW));
  const origin = { x: rect.x + spacing.pad, y: rect.y + spacing.pad };
  const placed =
    arrangement === 'auto' ? packRows(boxes, origin, spacing) : arrangeGrid(origin, boxes, GRID_COLUMNS[arrangement](boxes.length), spacing);
  return { tiles: placed, frames: new Map([[frame.id, frameAroundPlaced(rect, placed, spacing)]]) };
};

interface PlanFrameReorderArgs {
  id: string;
  arrangement: FrameArrangement;
  cellW: number;
  tiles: Tile[];
  frames: Frame[];
  spacing: Spacing;
}

export const planFrameReorder = ({ id, cellW, tiles, frames, spacing, arrangement }: PlanFrameReorderArgs): ReorderPlan | null => {
  const frame = frames.find((f) => f.id === id);
  if (!frame) return null;
  const members = (groupByFrame(frames, tiles).members.get(id) ?? []).filter(isMovable);
  if (!members.length) return null;
  return planFrame({ frame, members, spacing, cellW, arrangement });
};

interface PlanCanvasReorderArgs {
  tiles: Tile[];
  frames: Frame[];
  cellW: number;
  spacing: Spacing;
}

interface CanvasBlocks {
  blocks: Box[];
  tileRects: Map<string, Rect>;
  frameRects: Map<string, Rect>;
  memberIds: Map<string, string[]>;
}

const collectBlocks = ({ tiles, frames, cellW, spacing }: PlanCanvasReorderArgs): CanvasBlocks => {
  const { members, loose } = groupByFrame(frames, tiles);
  const tileRects = new Map<string, Rect>();
  const frameRects = new Map<string, Rect>();
  const memberIds = new Map<string, string[]>();
  const blocks: Box[] = [];

  for (const frame of frames) {
    const plan = planFrame({ frame, members: (members.get(frame.id) ?? []).filter(isMovable), spacing, cellW, arrangement: 'auto' });
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
  return { blocks, tileRects, frameRects, memberIds };
};

const moveBlocks = ({ blocks, tileRects, frameRects, memberIds }: CanvasBlocks, placedBlocks: Map<string, Rect>): ReorderPlan => {
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

export const planCanvasReorder = (args: PlanCanvasReorderArgs): ReorderPlan | null => {
  const collected = collectBlocks(args);
  const { blocks } = collected;
  if (!blocks.length) return null;
  const origin = { x: toGrid(Math.min(...blocks.map((b) => b.x))), y: toGrid(Math.min(...blocks.map((b) => b.y - b.lead))) };
  return moveBlocks(collected, packRows(blocks, origin, args.spacing));
};

const outerHeight = (box: Box): number => box.lead + box.height;

const wrapRows = (boxes: Box[], maxWidth: number, gap: number): Box[][] => {
  const rows: Box[][] = [];
  let width = 0;
  for (const box of boxes) {
    const row = rows[rows.length - 1];
    if (row && width + gap + box.width <= maxWidth) {
      row.push(box);
      width += gap + box.width;
      continue;
    }
    rows.push([box]);
    width = box.width;
  }
  return rows;
};

const packCentered = (boxes: Box[], aspect: number, spacing: Spacing): Map<string, Rect> => {
  const { gap } = spacing;
  const edge = edgeOf(spacing);
  const area = boxes.reduce((sum, b) => sum + (b.width + gap) * (outerHeight(b) + gap), 0);
  const maxWidth = Math.max(...boxes.map((b) => b.width), Math.sqrt(area * aspect));
  const rows = wrapRows(readingRows(boxes).flat(), maxWidth, gap);
  const rowWidth = (row: Box[]): number => row.reduce((sum, b) => sum + b.width, 0) + gap * (row.length - 1);
  const rowHeight = (row: Box[]): number => Math.max(...row.map(outerHeight));
  const clusterWidth = Math.max(...rows.map(rowWidth));
  const clusterHeight = rows.reduce((sum, row) => sum + rowHeight(row), 0) + gap * (rows.length - 1);
  const left = Math.min(...boxes.map((b) => b.x));
  const top = Math.min(...boxes.map((b) => b.y - b.lead));
  const right = Math.max(...boxes.map((b) => b.x + b.width));
  const bottom = Math.max(...boxes.map((b) => b.y + b.height));
  const originX = toGrid((left + right - clusterWidth) / 2);
  let y = toGrid((top + bottom - clusterHeight) / 2);
  const placed = new Map<string, Rect>();
  for (const row of rows) {
    const height = rowHeight(row);
    let x = edge(originX + (clusterWidth - rowWidth(row)) / 2);
    for (const box of row) {
      const boxY = edge(y + (height - outerHeight(box)) / 2 + box.lead);
      placed.set(box.id, { x, y: boxY, width: box.width, height: box.height });
      x = edge(x + box.width + gap);
    }
    y = edge(y + height + gap);
  }
  return placed;
};

interface PlanCanvasClusterArgs extends PlanCanvasReorderArgs {
  aspect: number;
}

export const planCanvasCluster = ({ aspect, ...args }: PlanCanvasClusterArgs): ReorderPlan | null => {
  const collected = collectBlocks(args);
  if (!collected.blocks.length) return null;
  return moveBlocks(collected, packCentered(collected.blocks, aspect, args.spacing));
};
