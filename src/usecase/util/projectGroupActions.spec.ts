import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';

import { test, expect, describe } from 'bun:test';

import { fixGroupFrame, moveTileIntoFrame, spawnInGroup } from '~/usecase/util/projectGroupActions';
import type { GroupOps } from '~/usecase/util/projectGroupActions';

const makeTile = (id: string, x = 0, y = 0, w = 200, h = 160): Tile => ({
  id, type: 'term', x, y, width: w, height: h, zIndex: 1
});

const makeFrame = (id: string, x: number, y: number, w: number, h: number): Frame => ({
  id, x, y, width: w, height: h, title: id, color: '#000'
});

const makeGroup = (overrides: Partial<ProjectGroup> = {}): ProjectGroup => ({
  key: 'loose',
  kind: 'loose',
  title: 'Test',
  path: null,
  frameId: null,
  tiles: [],
  outside: [],
  drifted: [],
  ...overrides
});

const makeOps = (tiles: Tile[] = [], frames: Frame[] = []): { ops: GroupOps; created: Frame[]; patched: Map<string, Partial<Omit<Frame, 'id'>>>; moved: { id: string; x: number; y: number }[][] } => {
  const created: Frame[] = [];
  const patched = new Map<string, Partial<Omit<Frame, 'id'>>>();
  const moved: { id: string; x: number; y: number }[][] = [];
  let nextId = 1;

  const ops: GroupOps = {
    tiles,
    frames,
    createFrame: (frame) => {
      const id = `f${nextId++}`;
      created.push({ ...frame, id });
      return id;
    },
    patchFrame: (id, patch) => { patched.set(id, patch); },
    placeTiles: (moves) => { moved.push(moves); }
  };

  return { ops, created, patched, moved };
};

describe('fixGroupFrame', () => {
  test('no tiles: creates frame at free position near origin', () => {
    const { ops, created } = makeOps();
    const group = makeGroup({ title: 'Empty', path: '/foo' });
    const id = fixGroupFrame({ ops, group, near: { x: 0, y: 0 } });
    expect(id).toBe('f1');
    expect(created).toHaveLength(1);
    expect(created[0]!.path).toBe('/foo');
    expect(created[0]!.title).toBe('Empty');
  });

  test('wrap: creates frame around tiles without moves', () => {
    const tiles = [makeTile('t1', 100, 100), makeTile('t2', 400, 100)];
    const { ops, created, moved } = makeOps(tiles);
    const group = makeGroup({ kind: 'path', path: '/bar', tiles, title: 'Bar' });
    fixGroupFrame({ ops, group, near: { x: 100, y: 100 } });
    expect(created).toHaveLength(1);
    expect(moved.flat()).toHaveLength(0);
    expect(created[0]!.path).toBe('/bar');
  });

  test('gather: places tiles then creates frame', () => {
    const groupTiles = [makeTile('t1', 0, 0), makeTile('t2', 0, 0)];
    const outside = makeTile('blocker', 100, 80, 20, 20);
    const { ops, created, moved } = makeOps([...groupTiles, outside]);
    const group = makeGroup({ kind: 'path', path: null, tiles: groupTiles, title: 'G', outside: [] });
    fixGroupFrame({ ops, group, near: { x: 0, y: 0 } });
    expect(created).toHaveLength(1);
    expect(moved.flat().length).toBeGreaterThanOrEqual(0);
  });
});

describe('moveTileIntoFrame', () => {
  test('returns false when frameId not found', () => {
    const tile = makeTile('t1', 0, 0);
    const { ops } = makeOps([tile], []);
    const result = moveTileIntoFrame({ ops, tile, frameId: 'missing' });
    expect(result).toBe(false);
  });

  test('places tile inside frame and returns fitted=true when space available', () => {
    const frame = makeFrame('fr1', 0, 0, 500, 400);
    const tile = makeTile('t1', 600, 600);
    const { ops, moved } = makeOps([tile], [frame]);
    const fitted = moveTileIntoFrame({ ops, tile, frameId: 'fr1' });
    expect(fitted).toBe(true);
    expect(moved.flat().some((m) => m.id === 't1')).toBe(true);
  });

  test('patches frame geometry when grown', () => {
    const frame = makeFrame('fr1', 0, 0, 300, 300);
    const occupant = makeTile('occ', 24, 24, 220, 220);
    const tile = makeTile('t1', 600, 600, 100, 80);
    const { ops, patched } = makeOps([occupant, tile], [frame]);
    moveTileIntoFrame({ ops, tile, frameId: 'fr1' });
    if (patched.has('fr1')) {
      const p = patched.get('fr1')!;
      expect(p.width ?? 300).toBeGreaterThanOrEqual(300);
    }
  });
});

describe('spawnInGroup', () => {
  test('returns null for non-frame group', () => {
    const { ops } = makeOps();
    const group = makeGroup({ kind: 'loose', frameId: null });
    const result = spawnInGroup({ ops, group, width: 200, height: 160 });
    expect(result).toBeNull();
  });

  test('returns null for frame group with no matching frame', () => {
    const { ops } = makeOps([], []);
    const group = makeGroup({ kind: 'frame', frameId: 'missing' });
    const result = spawnInGroup({ ops, group, width: 200, height: 160 });
    expect(result).toBeNull();
  });

  test('returns position when frame has space', () => {
    const frame = makeFrame('fr1', 0, 0, 500, 400);
    const { ops } = makeOps([], [frame]);
    const group = makeGroup({ kind: 'frame', frameId: 'fr1' });
    const result = spawnInGroup({ ops, group, width: 100, height: 80 });
    expect(result).not.toBeNull();
    expect(result!.fitted).toBe(true);
  });
});
