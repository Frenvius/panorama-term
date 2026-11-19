import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { captureLayout, layoutChanged, restoreLayout } from '~/usecase/util/layoutHistory';

const makeTile = (id: string, x: number, y: number): Tile => ({ id, type: 'term', x, y, width: 200, height: 160, zIndex: 1 });

const makeFrame = (id: string, x: number, title = 'Frame'): Frame => ({ id, x, y: 0, width: 400, height: 300, title, color: '#fff' });

describe('layoutChanged', () => {
  test('ignores added and removed tiles', () => {
    const before = captureLayout({ tiles: [makeTile('a', 0, 0)], frames: [] });
    const after = captureLayout({ tiles: [makeTile('b', 40, 40)], frames: [] });
    expect(layoutChanged(before, after)).toBe(false);
  });

  test('detects a moved tile', () => {
    const before = captureLayout({ tiles: [makeTile('a', 0, 0)], frames: [] });
    const after = captureLayout({ tiles: [makeTile('a', 16, 0)], frames: [] });
    expect(layoutChanged(before, after)).toBe(true);
  });

  test('detects frame edits by value', () => {
    const before = captureLayout({ tiles: [], frames: [makeFrame('f', 0)] });
    expect(layoutChanged(before, captureLayout({ tiles: [], frames: [makeFrame('f', 0)] }))).toBe(false);
    expect(layoutChanged(before, captureLayout({ tiles: [], frames: [makeFrame('f', 0, 'Renamed')] }))).toBe(true);
    expect(layoutChanged(before, captureLayout({ tiles: [], frames: [] }))).toBe(true);
  });
});

describe('restoreLayout', () => {
  test('restores geometry and frames, keeps tiles absent from the snapshot', () => {
    const snapshot = captureLayout({ tiles: [makeTile('a', 0, 0)], frames: [makeFrame('f', 0)] });
    const current = { tiles: [makeTile('a', 80, 80), makeTile('b', 400, 0)], frames: [] };
    const next = restoreLayout(snapshot, current);
    expect(next?.tiles.map((t) => [t.id, t.x, t.y])).toEqual([
      ['a', 0, 0],
      ['b', 400, 0]
    ]);
    expect(next?.frames.map((f) => f.id)).toEqual(['f']);
  });

  test('returns null when nothing would change', () => {
    const snapshot = captureLayout({ tiles: [makeTile('gone', 0, 0)], frames: [] });
    expect(restoreLayout(snapshot, { tiles: [makeTile('a', 80, 80)], frames: [] })).toBeNull();
  });
});
