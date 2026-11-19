import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { planFrameReorder, planCanvasReorder } from '~/usecase/util/reorderLayout';

const makeTile = (id: string, x: number, y: number, w = 200, h = 160): Tile => ({ id, type: 'term', x, y, width: w, height: h, zIndex: 1 });

const makeFrame = (id: string, x: number, y: number, w: number, h: number): Frame => ({
  id,
  x,
  y,
  width: w,
  height: h,
  title: id,
  color: '#000'
});

const rect = (x: number, y: number, width = 200, height = 160) => ({ x, y, width, height });

describe('planFrameReorder', () => {
  test('keeps reading order and closes gaps on the grid in default mode', () => {
    const frame = makeFrame('f', 0, 0, 2000, 1000);
    const tiles = [makeTile('b', 900, 40), makeTile('a', 100, 60), makeTile('c', 300, 500)];
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], mode: 'default' })!;
    expect(plan.tiles.get('a')).toEqual(rect(20, 20));
    expect(plan.tiles.get('b')).toEqual(rect(240, 20));
    expect(plan.tiles.get('c')).toEqual(rect(20, 200));
    expect(plan.frames.get('f')).toEqual(rect(0, 0, 460, 380));
  });

  test('compact mode leaves no gap and no padding', () => {
    const frame = makeFrame('f', 0, 0, 2000, 1000);
    const tiles = [makeTile('a', 100, 60), makeTile('b', 900, 40), makeTile('c', 300, 500)];
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], mode: 'compact' })!;
    expect(plan.tiles.get('a')).toEqual(rect(0, 0));
    expect(plan.tiles.get('b')).toEqual(rect(200, 0));
    expect(plan.tiles.get('c')).toEqual(rect(0, 160));
    expect(plan.frames.get('f')).toEqual(rect(0, 0, 400, 320));
  });

  test('puts off-grid frames and tile sizes back on the grid', () => {
    const frame = makeFrame('f', 13, 27, 900, 600);
    const tiles = [makeTile('a', 40, 60, 207, 163), makeTile('b', 400, 60, 200, 160)];
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], mode: 'compact' })!;
    expect(plan.tiles.get('a')).toEqual(rect(20, 20, 200, 160));
    expect(plan.tiles.get('b')).toEqual(rect(220, 20));
    expect(plan.frames.get('f')).toEqual(rect(20, 20, 400, 160));
  });

  test('skips pinned tiles and empty frames', () => {
    const frame = makeFrame('f', 0, 0, 1000, 1000);
    const pinned = { ...makeTile('p', 300, 300), pinned: true };
    expect(planFrameReorder({ id: 'f', tiles: [pinned], frames: [frame], mode: 'compact' })).toBeNull();
  });
});

describe('planCanvasReorder', () => {
  test('compact mode stacks frames edge to edge and moves members along', () => {
    const f1 = makeFrame('f1', 0, 28, 500, 300);
    const f2 = makeFrame('f2', 0, 1000, 500, 300);
    const tiles = [makeTile('a', 50, 80), makeTile('b', 50, 1050), makeTile('loose', 2000, 40)];
    const plan = planCanvasReorder({ tiles, frames: [f1, f2], mode: 'compact' })!;
    expect(plan.frames.get('f1')).toEqual(rect(0, 20));
    expect(plan.tiles.get('a')).toEqual(rect(0, 20));
    expect(plan.tiles.get('loose')).toEqual(rect(200, 20));
    expect(plan.frames.get('f2')).toEqual(rect(0, 180));
    expect(plan.tiles.get('b')).toEqual(rect(0, 180));
  });

  test('default mode reserves the frame bar above each frame on the grid', () => {
    const f1 = makeFrame('f1', 0, 100, 500, 300);
    const f2 = makeFrame('f2', 0, 1000, 500, 300);
    const tiles = [makeTile('a', 50, 150), makeTile('b', 50, 1050)];
    const plan = planCanvasReorder({ tiles, frames: [f1, f2], mode: 'default' })!;
    expect(plan.frames.get('f1')).toEqual(rect(0, 120, 240, 200));
    expect(plan.tiles.get('a')).toEqual(rect(20, 140));
    expect(plan.frames.get('f2')).toEqual(rect(0, 360, 240, 200));
  });

  test('returns null for an empty canvas', () => {
    expect(planCanvasReorder({ tiles: [], frames: [], mode: 'default' })).toBeNull();
  });
});
