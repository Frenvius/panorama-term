import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { CELL } from '~/usecase/util/constants';
import { layoutSpacing, planFrameReorder, planCanvasCluster, planCanvasReorder } from '~/usecase/util/reorderLayout';

const SPACED = layoutSpacing('default', CELL);
const COMPACT = layoutSpacing('compact', CELL);

const CELL_W = 7.23;

const makeTile = (id: string, x: number, y: number, w = 200, h = 160): Tile => ({ id, type: 'note', x, y, width: w, height: h, zIndex: 1 });

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
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], arrangement: 'auto', cellW: CELL_W, spacing: SPACED })!;
    expect(plan.tiles.get('a')).toEqual(rect(20, 20));
    expect(plan.tiles.get('b')).toEqual(rect(240, 20));
    expect(plan.tiles.get('c')).toEqual(rect(20, 200));
    expect(plan.frames.get('f')).toEqual(rect(0, 0, 460, 380));
  });

  test('compact mode leaves no gap and no padding', () => {
    const frame = makeFrame('f', 0, 0, 2000, 1000);
    const tiles = [makeTile('a', 100, 60), makeTile('b', 900, 40), makeTile('c', 300, 500)];
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], arrangement: 'auto', cellW: CELL_W, spacing: COMPACT })!;
    expect(plan.tiles.get('a')).toEqual(rect(0, 0));
    expect(plan.tiles.get('b')).toEqual(rect(200, 0));
    expect(plan.tiles.get('c')).toEqual(rect(0, 160));
    expect(plan.frames.get('f')).toEqual(rect(0, 0, 400, 320));
  });

  test('puts off-grid frames and tile sizes back on the grid', () => {
    const frame = makeFrame('f', 13, 27, 900, 600);
    const tiles = [makeTile('a', 40, 60, 207, 163), makeTile('b', 400, 60, 200, 160)];
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], arrangement: 'auto', cellW: CELL_W, spacing: COMPACT })!;
    expect(plan.tiles.get('a')).toEqual(rect(20, 20, 200, 160));
    expect(plan.tiles.get('b')).toEqual(rect(220, 20));
    expect(plan.frames.get('f')).toEqual(rect(20, 20, 400, 160));
  });

  test('compact mode abuts terminal tiles of uneven snapped size and wraps the frame exactly', () => {
    const frame = makeFrame('f', 0, 0, 2000, 2000);
    const term = (id: string, y: number, w: number, h: number): Tile => ({ ...makeTile(id, 0, y, w, h), type: 'term' });
    const tiles = [term('a', 0, 600, 500), term('b', 600, 650, 400)];
    const plan = planFrameReorder({ id: 'f', tiles, frames: [frame], arrangement: 'auto', cellW: CELL_W, spacing: COMPACT })!;
    const a = plan.tiles.get('a')!;
    const b = plan.tiles.get('b')!;
    const f = plan.frames.get('f')!;
    expect(b.y).toBe(a.y + a.height);
    expect(f.width).toBe(Math.max(a.width, b.width));
    expect(f.height).toBe(b.y + b.height);
  });

  test('arrangements keep tile sizes and wrap the frame around them', () => {
    const frame = makeFrame('f', 0, 0, 2000, 2000);
    const tiles = [makeTile('a', 0, 0), makeTile('b', 300, 0, 300, 140), makeTile('c', 0, 300), makeTile('d', 300, 300)];
    const plan = (arrangement: 'horizontal' | 'vertical' | 'grid2', spacing = COMPACT) =>
      planFrameReorder({ id: 'f', tiles, frames: [frame], arrangement, cellW: CELL_W, spacing })!;
    expect(plan('horizontal').tiles.get('d')).toEqual(rect(700, 0));
    expect(plan('horizontal').frames.get('f')).toEqual(rect(0, 0, 900, 160));
    expect(plan('vertical').tiles.get('c')).toEqual(rect(0, 300));
    expect(plan('vertical').frames.get('f')).toEqual(rect(0, 0, 300, 620));
    expect(plan('grid2').tiles.get('b')).toEqual(rect(200, 0, 300, 140));
    expect(plan('grid2').tiles.get('d')).toEqual(rect(200, 160));
    expect(plan('grid2').frames.get('f')).toEqual(rect(0, 0, 500, 320));
    expect(plan('grid2', SPACED).tiles.get('d')).toEqual(rect(240, 200));
  });

  test('skips pinned tiles and empty frames', () => {
    const frame = makeFrame('f', 0, 0, 1000, 1000);
    const pinned = { ...makeTile('p', 300, 300), pinned: true };
    expect(planFrameReorder({ id: 'f', tiles: [pinned], frames: [frame], arrangement: 'auto', cellW: CELL_W, spacing: COMPACT })).toBeNull();
  });
});

describe('planCanvasReorder', () => {
  test('compact mode stacks frames edge to edge and moves members along', () => {
    const f1 = makeFrame('f1', 0, 28, 500, 300);
    const f2 = makeFrame('f2', 0, 1000, 500, 300);
    const tiles = [makeTile('a', 50, 80), makeTile('b', 50, 1050), makeTile('loose', 2000, 40)];
    const plan = planCanvasReorder({ tiles, frames: [f1, f2], cellW: CELL_W, spacing: COMPACT })!;
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
    const plan = planCanvasReorder({ tiles, frames: [f1, f2], cellW: CELL_W, spacing: SPACED })!;
    expect(plan.frames.get('f1')).toEqual(rect(0, 120, 240, 200));
    expect(plan.tiles.get('a')).toEqual(rect(20, 140));
    expect(plan.frames.get('f2')).toEqual(rect(0, 360, 240, 200));
  });

  test('returns null for an empty canvas', () => {
    expect(planCanvasReorder({ tiles: [], frames: [], cellW: CELL_W, spacing: SPACED })).toBeNull();
  });
});

describe('locked frame order', () => {
  test('planCanvasReorder keeps member relative offsets inside a locked frame', () => {
    const f1 = makeFrame('f1', 0, 0, 500, 500);
    const f1Locked = { ...f1, orderLocked: true };
    const tiles = [makeTile('top', 20, 40), makeTile('bot', 20, 240)];
    const plan = planCanvasReorder({ tiles, frames: [f1Locked], cellW: CELL_W, spacing: COMPACT })!;
    const top = plan.tiles.get('top')!;
    const bot = plan.tiles.get('bot')!;
    expect(bot.x - top.x).toBe(0);
    expect(bot.y - top.y).toBe(200);
  });

  test('planCanvasReorder reflowed unlocked frame compacts members to origin, changing relative offsets', () => {
    const f1 = makeFrame('f1', 0, 0, 500, 500);
    const tiles = [makeTile('top', 20, 40), makeTile('bot', 20, 240)];
    const plan = planCanvasReorder({ tiles, frames: [f1], cellW: CELL_W, spacing: COMPACT })!;
    const top = plan.tiles.get('top')!;
    const bot = plan.tiles.get('bot')!;
    expect(top.x).toBe(0);
    expect(top.y).toBe(0);
    expect(bot.y - top.y).toBe(160);
  });

  test('planCanvasCluster keeps member relative offsets inside a locked frame', () => {
    const f1 = makeFrame('f1', 0, 0, 500, 500);
    const f1Locked = { ...f1, orderLocked: true };
    const tiles = [makeTile('top', 20, 40), makeTile('bot', 20, 240)];
    const plan = planCanvasCluster({ tiles, frames: [f1Locked], aspect: 16 / 9, cellW: CELL_W, spacing: COMPACT })!;
    const top = plan.tiles.get('top')!;
    const bot = plan.tiles.get('bot')!;
    expect(bot.x - top.x).toBe(0);
    expect(bot.y - top.y).toBe(200);
  });

  test('planCanvasCluster drops leading slack inside a locked frame so it packs flush', () => {
    const f1Locked = { ...makeFrame('f1', 0, 0, 213, 173), orderLocked: true };
    const plan = planCanvasCluster({ tiles: [makeTile('a', 5, 13)], frames: [f1Locked], aspect: 1, cellW: CELL_W, spacing: COMPACT })!;
    const frame = plan.frames.get('f1')!;
    const tile = plan.tiles.get('a')!;
    expect(tile.x - frame.x).toBe(0);
    expect(tile.y - frame.y).toBe(0);
    expect(frame.width).toBe(200);
    expect(frame.height).toBe(160);
  });
});

describe('planCanvasCluster', () => {
  test('wraps a vertical stack into rows centered on the original bounds', () => {
    const tiles = [makeTile('a', 0, 0), makeTile('b', 0, 160), makeTile('c', 0, 320), makeTile('d', 0, 480)];
    const plan = planCanvasCluster({ tiles, frames: [], aspect: 16 / 9, cellW: CELL_W, spacing: COMPACT })!;
    expect(plan.tiles.get('a')).toEqual(rect(-100, 160));
    expect(plan.tiles.get('b')).toEqual(rect(100, 160));
    expect(plan.tiles.get('c')).toEqual(rect(-100, 320));
    expect(plan.tiles.get('d')).toEqual(rect(100, 320));
  });

  test('fills the space beside a tall block instead of leaving a gap', () => {
    const tiles = [makeTile('tall', 0, 0, 200, 480), makeTile('a', 0, 500), makeTile('b', 0, 700), makeTile('c', 0, 900)];
    const plan = planCanvasCluster({ tiles, frames: [], aspect: 1, cellW: CELL_W, spacing: COMPACT })!;
    const tall = plan.tiles.get('tall')!;
    const ys = ['a', 'b', 'c'].map((id) => plan.tiles.get(id)!.y - tall.y).sort((p, q) => p - q);
    expect(ys).toEqual([0, 160, 320]);
    expect(['a', 'b', 'c'].every((id) => plan.tiles.get(id)!.x === tall.x + 200)).toBe(true);
  });

  test('reflows a long single-row frame into a balanced grid', () => {
    const frame = makeFrame('f', 0, 0, 1400, 160);
    const tiles = Array.from({ length: 6 }, (_, i) => makeTile(`t${i}`, i * 200, 0));
    const plan = planCanvasCluster({ tiles, frames: [frame], aspect: 16 / 9, cellW: CELL_W, spacing: COMPACT })!;
    expect(plan.frames.get('f')!.width).toBe(600);
    expect(plan.frames.get('f')!.height).toBe(320);
  });

  test('picks the column count that leaves no empty cell in a frame', () => {
    const frame = makeFrame('f', 0, 0, 200, 480);
    const tiles = [makeTile('a', 0, 0), makeTile('b', 0, 160), makeTile('c', 0, 320)];
    const plan = planCanvasCluster({ tiles, frames: [frame], aspect: 16 / 9, cellW: CELL_W, spacing: COMPACT })!;
    expect(plan.frames.get('f')!.width).toBe(600);
    expect(plan.frames.get('f')!.height).toBe(160);
  });

  test('never overlaps blocks in spaced mode', () => {
    const f1 = makeFrame('f1', 0, 0, 700, 900);
    const f2 = makeFrame('f2', 800, 0, 1500, 200);
    const tiles = [
      ...Array.from({ length: 9 }, (_, i) => makeTile(`a${i}`, (i % 3) * 220 + 20, Math.floor(i / 3) * 180 + 20)),
      ...Array.from({ length: 7 }, (_, i) => makeTile(`b${i}`, 820 + i * 210, 20)),
      makeTile('loose', 3000, 0, 300, 600)
    ];
    const plan = planCanvasCluster({ tiles, frames: [f1, f2], aspect: 16 / 9, cellW: CELL_W, spacing: SPACED })!;
    const blocks = [...plan.frames.values(), plan.tiles.get('loose')!];
    const overlaps = blocks.some((p, i) =>
      blocks.some((q, j) => i < j && p.x < q.x + q.width && q.x < p.x + p.width && p.y < q.y + q.height && q.y < p.y + p.height)
    );
    expect(overlaps).toBe(false);
  });

  test('returns null for an empty canvas', () => {
    expect(planCanvasCluster({ tiles: [], frames: [], aspect: 1, cellW: CELL_W, spacing: SPACED })).toBeNull();
  });
});
