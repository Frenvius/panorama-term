import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { frameOf, tileInFrame, groupByFrame } from '~/usecase/util/frame';

const makeTile = (id: string, x: number, y: number, w = 100, h = 80): Tile => ({
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

describe('tileInFrame', () => {
  test('true when tile center is inside frame', () => {
    const frame = makeFrame('f1', 0, 0, 200, 200);
    const tile = makeTile('t1', 50, 50);
    expect(tileInFrame(frame, tile)).toBe(true);
  });

  test('false when tile center is outside frame', () => {
    const frame = makeFrame('f1', 0, 0, 100, 100);
    const tile = makeTile('t1', 150, 0);
    expect(tileInFrame(frame, tile)).toBe(false);
  });

  test('boards belong only to frames with the same normalized repository', () => {
    const board = { ...makeTile('board', 50, 50), type: 'orch' as const, cwd: 'D:\\repo\\' };
    const frame = { ...makeFrame('f1', 0, 0, 200, 200), path: 'd:/REPO' };
    expect(tileInFrame(frame, board)).toBe(true);
    expect(tileInFrame({ ...frame, path: 'd:/other' }, board)).toBe(false);
    expect(tileInFrame({ ...frame, path: undefined }, board)).toBe(false);
    expect(frameOf([{ ...frame, path: 'd:/other' }], board)).toBeNull();
  });
});

describe('frameOf', () => {
  test('returns null when no frame contains the tile center', () => {
    const frames = [makeFrame('f1', 0, 0, 100, 100)];
    const tile = makeTile('t1', 200, 200);
    expect(frameOf(frames, tile)).toBeNull();
  });

  test('returns the only containing frame', () => {
    const frames = [makeFrame('f1', 0, 0, 200, 200)];
    const tile = makeTile('t1', 50, 50);
    expect(frameOf(frames, tile)?.id).toBe('f1');
  });

  test('smallest area wins when multiple frames contain the tile center', () => {
    const large = makeFrame('large', 0, 0, 400, 400);
    const small = makeFrame('small', 50, 50, 100, 100);
    const tile = makeTile('t1', 70, 70, 20, 20);
    expect(frameOf([large, small], tile)?.id).toBe('small');
    expect(frameOf([small, large], tile)?.id).toBe('small');
  });

  test('tie (equal area) resolves to later frame in array', () => {
    const f1 = makeFrame('f1', 0, 0, 200, 200);
    const f2 = makeFrame('f2', 10, 10, 200, 200);
    const tile = makeTile('t1', 100, 100, 20, 20);
    expect(frameOf([f1, f2], tile)?.id).toBe('f2');
  });

  test('returns null for empty frames array', () => {
    const tile = makeTile('t1', 0, 0);
    expect(frameOf([], tile)).toBeNull();
  });
});

describe('groupByFrame', () => {
  test('assigns tiles to smallest containing frame', () => {
    const large = makeFrame('large', 0, 0, 400, 400);
    const small = makeFrame('small', 50, 50, 100, 100);
    const inSmall = makeTile('t1', 70, 70, 20, 20);
    const inLargeOnly = makeTile('t2', 300, 300, 20, 20);
    const outside = makeTile('t3', 500, 500, 20, 20);

    const { members, loose } = groupByFrame([large, small], [inSmall, inLargeOnly, outside]);
    expect(members.get('small')).toEqual([inSmall]);
    expect(members.get('large')).toEqual([inLargeOnly]);
    expect(loose).toEqual([outside]);
  });
});
