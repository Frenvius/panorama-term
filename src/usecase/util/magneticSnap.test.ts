import type { Tile } from '~/domain/interfaces/workspace.interface';

import { expect, test } from 'bun:test';

import { computeResizeSnap } from '~/usecase/util/magneticSnap';

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
