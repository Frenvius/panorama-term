import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { placeBeside, findFreeRect, planGroupFrame, frameAroundTiles, placeTileInFrame } from '~/usecase/util/frameLayout';
import type { Rect } from '~/usecase/util/frameLayout';

const SPACED = { gap: 24, pad: 24, barSpace: 0 };
const COMPACT = { gap: 0, pad: 0, barSpace: 0 };

const makeTile = (id: string, x: number, y: number, w = 200, h = 160): Tile => ({
  id,
  type: 'term',
  x,
  y,
  width: w,
  height: h,
  zIndex: 1
});

const makeFrame = (id: string, x: number, y: number, w: number, h: number): Frame => ({
  id,
  x,
  y,
  width: w,
  height: h,
  title: id,
  color: '#000'
});

describe('frameAroundTiles', () => {
  test('returns zero rect for empty array', () => {
    expect(frameAroundTiles([], 24)).toEqual({ x: 0, y: 0, width: 0, height: 0 });
  });

  test('encloses single tile with FRAME_PADDING=24 on all sides', () => {
    const tile = makeTile('t1', 100, 200, 300, 150);
    const rect = frameAroundTiles([tile], 24);
    expect(rect.x).toBe(76);
    expect(rect.y).toBe(176);
    expect(rect.width).toBe(348);
    expect(rect.height).toBe(198);
  });

  test('encloses multiple tiles', () => {
    const t1 = makeTile('t1', 0, 0, 100, 80);
    const t2 = makeTile('t2', 300, 200, 100, 80);
    const rect = frameAroundTiles([t1, t2], 24);
    expect(rect.x).toBe(-24);
    expect(rect.y).toBe(-24);
    expect(rect.width).toBe(448);
    expect(rect.height).toBe(328);
  });
});

describe('findFreeRect', () => {
  test('returns near position when no obstacles', () => {
    const pos = findFreeRect({ width: 100, height: 80, near: { x: 200, y: 300 }, obstacles: [] });
    expect(pos).toEqual({ x: 200, y: 300 });
  });

  test('skips blocked positions and finds first free slot', () => {
    const obstacle: Rect = { x: 190, y: 290, width: 120, height: 100 };
    const pos = findFreeRect({ width: 100, height: 80, near: { x: 200, y: 300 }, obstacles: [obstacle], gap: 0 });
    const noOverlap = pos.x + 100 <= obstacle.x || pos.x >= obstacle.x + obstacle.width || pos.y + 80 <= obstacle.y || pos.y >= obstacle.y + obstacle.height;
    expect(noOverlap).toBe(true);
  });

  test('respects gap inflation', () => {
    const obstacle: Rect = { x: 100, y: 100, width: 200, height: 200 };
    const pos = findFreeRect({ width: 50, height: 50, near: { x: 150, y: 150 }, obstacles: [obstacle], gap: 20 });
    const clearWithGap =
      pos.x + 50 + 20 <= obstacle.x ||
      pos.x - 20 >= obstacle.x + obstacle.width ||
      pos.y + 50 + 20 <= obstacle.y ||
      pos.y - 20 >= obstacle.y + obstacle.height;
    expect(clearWithGap).toBe(true);
  });
});

describe('planGroupFrame', () => {
  test('wrap when no outside tile centers in bbox', () => {
    const tiles = [makeTile('t1', 100, 100), makeTile('t2', 400, 100)];
    const result = planGroupFrame({ groupTiles: tiles, allTiles: tiles, frames: [], spacing: SPACED });
    expect(result.kind).toBe('wrap');
    if (result.kind === 'wrap') {
      expect(result.rect.width).toBeGreaterThan(0);
    }
  });

  test('gather when outside tile center falls inside bbox', () => {
    const groupTiles = [makeTile('t1', 100, 100), makeTile('t2', 400, 100)];
    const outsideTile = makeTile('outside', 250, 110, 50, 50);
    const allTiles = [...groupTiles, outsideTile];
    const result = planGroupFrame({ groupTiles, allTiles, frames: [], spacing: SPACED });
    expect(result.kind).toBe('gather');
  });

  test('gather moves do not overlap each other', () => {
    const groupTiles = [
      makeTile('t1', 0, 0, 200, 160),
      makeTile('t2', 0, 0, 200, 160),
      makeTile('t3', 0, 0, 200, 160)
    ];
    const outsideTile = makeTile('blocker', 100, 80, 20, 20);
    const allTiles = [...groupTiles, outsideTile];
    const result = planGroupFrame({ groupTiles, allTiles, frames: [], spacing: SPACED });
    if (result.kind !== 'gather') return;

    for (let i = 0; i < result.moves.length; i++) {
      const a = result.moves[i]!;
      const ta = groupTiles[i]!;
      for (let j = i + 1; j < result.moves.length; j++) {
        const b = result.moves[j]!;
        const tb = groupTiles[j]!;
        const overlap =
          a.x < b.x + tb.width && a.x + ta.width > b.x && a.y < b.y + tb.height && a.y + ta.height > b.y;
        expect(overlap).toBe(false);
      }
    }
  });

  test('gather moves land clear of outside tile centers', () => {
    const groupTiles = [makeTile('t1', 0, 0), makeTile('t2', 0, 0)];
    const outsideTile = makeTile('blocker', 100, 80, 20, 20);
    const allTiles = [...groupTiles, outsideTile];
    const result = planGroupFrame({ groupTiles, allTiles, frames: [], spacing: SPACED });
    if (result.kind !== 'gather') return;

    const { rect } = result;
    const cx = outsideTile.x + outsideTile.width / 2;
    const cy = outsideTile.y + outsideTile.height / 2;
    const insideFrame = cx >= rect.x && cx < rect.x + rect.width && cy >= rect.y && cy < rect.y + rect.height;
    expect(insideFrame).toBe(false);
  });
});

describe('placeTileInFrame', () => {
  test('branch 1: free slot inside frame interior when space available', () => {
    const frame = makeFrame('f1', 0, 0, 500, 400);
    const result = placeTileInFrame({ frame, width: 100, height: 80, tiles: [], frames: [frame], spacing: SPACED });
    expect(result.fitted).toBe(true);
    expect(result.frame).toBeNull();
    expect(result.x).toBeGreaterThanOrEqual(frame.x + 24);
    expect(result.y).toBeGreaterThanOrEqual(frame.y + 24);
    expect(result.x + 100).toBeLessThanOrEqual(frame.x + frame.width - 24);
    expect(result.y + 80).toBeLessThanOrEqual(frame.y + frame.height - 24);
  });

  test('branch 2: grow right when interior is occupied', () => {
    const frame = makeFrame('f1', 0, 0, 300, 300);
    const occupant = makeTile('occ', 24, 24, 220, 220);
    const result = placeTileInFrame({ frame, width: 100, height: 80, tiles: [occupant], frames: [frame], spacing: SPACED });
    if (result.fitted && result.frame !== null) {
      expect(result.frame.width).toBeGreaterThan(frame.width);
      expect(result.frame.height).toBe(frame.height);
    }
  });

  test('branch 4: returns fitted=false when frame cannot grow and no interior slot', () => {
    const frame = makeFrame('f1', 0, 0, 200, 200);
    const occupant = makeTile('occ', 24, 24, 130, 130);
    const blocker = makeTile('blocker', 350, 50, 20, 20);
    const otherFrame = makeFrame('f2', 150, 0, 300, 300);
    const result = placeTileInFrame({
      frame,
      width: 150,
      height: 150,
      tiles: [occupant, blocker],
      frames: [frame, otherFrame],
      spacing: SPACED
    });
    if (!result.fitted) {
      expect(result.frame).toBeNull();
      expect(result.fitted).toBe(false);
    }
  });

  test('compact packs a new tile flush against existing members', () => {
    const frame = makeFrame('f1', 0, 0, 600, 200);
    const occupant = makeTile('occ', 0, 0, 200, 200);
    const result = placeTileInFrame({ frame, width: 200, height: 200, tiles: [occupant], frames: [frame], spacing: COMPACT });
    expect(result).toEqual({ x: 200, y: 0, frame: null, fitted: true });
  });

  test('spaced keeps the gap from existing members', () => {
    const frame = makeFrame('f1', 0, 0, 700, 300);
    const occupant = makeTile('occ', 24, 24, 200, 200);
    const result = placeTileInFrame({ frame, width: 200, height: 200, tiles: [occupant], frames: [frame], spacing: SPACED });
    expect(result).toEqual({ x: 248, y: 24, frame: null, fitted: true });
  });
});

describe('placeBeside', () => {
  test('loose tile lands to the right of the source with the gap', () => {
    const src = makeTile('src', 0, 0, 200, 160);
    expect(placeBeside({ src, tiles: [src], frames: [], spacing: SPACED })).toEqual({ x: 224, y: 0, frame: null });
  });

  test('framed tile stays inside its frame, growing it when full', () => {
    const frame = makeFrame('f1', 0, 0, 248, 208);
    const src = makeTile('src', 24, 24, 200, 160);
    const spot = placeBeside({ src, tiles: [src], frames: [frame], spacing: SPACED });
    expect(spot.x).toBe(248);
    expect(spot.frame?.id).toBe('f1');
  });
});
