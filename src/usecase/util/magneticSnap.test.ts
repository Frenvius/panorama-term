import type { Tile } from '~/domain/interfaces/workspace.interface';

import { expect, test } from 'bun:test';

import { settleRect, settleOffset, computeResizeSnap } from '~/usecase/util/magneticSnap';

const tile = (id: string, x: number, y: number, w: number, h: number): Tile => ({
  id,
  x,
  y,
  width: w,
  height: h,
  zIndex: 1,
  type: 'term'
});

test('east edge snaps to neighbor left edge', () => {
  const moving = tile('a', 0, 0, 195, 100);
  const other = tile('b', 200, 0, 100, 100);
  const snap = computeResizeSnap(moving, 'e', [other], 8, 50, 50);
  expect(snap.width).toBe(200);
});

test('west edge snap moves x and keeps right edge', () => {
  const moving = tile('a', 96, 0, 104, 100);
  const other = tile('b', 0, 0, 100, 100);
  const snap = computeResizeSnap(moving, 'w', [other], 8, 50, 50);
  expect(snap.x).toBe(100);
  expect(snap.x + snap.width).toBe(200);
});

test('snap rejected when it violates min size', () => {
  const moving = tile('a', 0, 0, 55, 100);
  const other = tile('b', 52, 0, 100, 100);
  const snap = computeResizeSnap(moving, 'e', [other], 8, 54, 50);
  expect(snap.width).toBe(55);
});

test('no snap outside the perpendicular band', () => {
  const moving = tile('a', 0, 0, 195, 100);
  const other = tile('b', 200, 500, 100, 100);
  const snap = computeResizeSnap(moving, 'e', [other], 8, 50, 50);
  expect(snap.width).toBe(195);
});

test('settleRect rounds a free rect to the grid', () => {
  const r = settleRect({ rect: { x: 13, y: 27, width: 207, height: 163 }, others: [], cell: 20, minW: 200, minH: 120 });
  expect(r).toEqual({ x: 20, y: 20, width: 200, height: 160 });
});

test('settleRect keeps an edge glued to an off-grid neighbour', () => {
  const neighbour = { x: 0, y: 0, width: 213, height: 160 };
  const r = settleRect({ rect: { x: 213, y: 7, width: 200, height: 160 }, others: [neighbour], cell: 20, minW: 200, minH: 120 });
  expect(r).toEqual({ x: 213, y: 0, width: 200, height: 160 });
});

test('settleOffset keeps a glued group in place and rounds a free one', () => {
  const neighbour = { x: 0, y: 0, width: 213, height: 160 };
  expect(settleOffset({ x: 213, y: 47, width: 400, height: 160 }, [neighbour], 20)).toEqual({ dx: 0, dy: -7 });
});
