import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { projectsInUse, buildProjectGroups, tileOwnerResolver } from '~/usecase/util/projectGroups';

const makeTile = (id: string, overrides: Partial<Tile> = {}): Tile => ({
  id,
  type: 'term',
  x: 0,
  y: 0,
  width: 100,
  height: 80,
  zIndex: 1,
  ...overrides
});

const makeFrame = (id: string, x: number, y: number, w: number, h: number, overrides: Partial<Frame> = {}): Frame => ({
  id,
  x,
  y,
  width: w,
  height: h,
  title: id,
  color: '#000',
  ...overrides
});

describe('buildProjectGroups - frame groups', () => {
  test('tile in smallest frame wins (tie: later frame in array)', () => {
    const large = makeFrame('large', 0, 0, 400, 400);
    const small = makeFrame('small', 50, 50, 100, 100);
    const tile = makeTile('t1', { x: 70, y: 70, width: 20, height: 20 });

    const groups = buildProjectGroups({ frames: [large, small], tiles: [tile], projects: [] });
    const largeGroup = groups.find((g) => g.frameId === 'large')!;
    const smallGroup = groups.find((g) => g.frameId === 'small')!;
    expect(smallGroup.tiles).toContain(tile);
    expect(largeGroup.tiles).toHaveLength(0);
  });

  test('explicit frame path is used over inferred', () => {
    const frame = makeFrame('f1', 0, 0, 400, 400, { path: '/explicit/path' });
    const tile = makeTile('t1', { x: 100, y: 100, cwd: '/other/path' });
    const groups = buildProjectGroups({ frames: [frame], tiles: [tile], projects: ['/explicit/path', '/other/path'] });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.path).toBe('/explicit/path');
  });

  test('inferred path = most frequent tileProject among member tiles', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000);
    const projects = ['/proj/a', '/proj/b'];
    const tiles = [
      makeTile('t1', { x: 100, y: 100, cwd: '/proj/a' }),
      makeTile('t2', { x: 200, y: 200, cwd: '/proj/a' }),
      makeTile('t3', { x: 300, y: 300, cwd: '/proj/b' })
    ];
    const groups = buildProjectGroups({ frames: [frame], tiles, projects });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.path).toBe('/proj/a');
  });

  test('inferred path tie resolves to first seen', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000);
    const projects = ['/proj/a', '/proj/b'];
    const tiles = [
      makeTile('t1', { x: 100, y: 100, cwd: '/proj/a' }),
      makeTile('t2', { x: 200, y: 200, cwd: '/proj/b' })
    ];
    const groups = buildProjectGroups({ frames: [frame], tiles, projects });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.path).toBe('/proj/a');
  });

  test('path is null when no member tiles match any project', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000);
    const tile = makeTile('t1', { x: 100, y: 100 });
    const groups = buildProjectGroups({ frames: [frame], tiles: [tile], projects: [] });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.path).toBeNull();
  });

  test('drifted contains member tiles whose tileProject differs from group path', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000);
    const projects = ['/proj/a', '/proj/b'];
    const tiles = [
      makeTile('t1', { x: 100, y: 100, cwd: '/proj/a' }),
      makeTile('t2', { x: 200, y: 200, cwd: '/proj/a' }),
      makeTile('t3', { x: 300, y: 300, cwd: '/proj/b' })
    ];
    const groups = buildProjectGroups({ frames: [frame], tiles, projects });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.drifted).toEqual(['t3']);
  });

  test('drifted is empty when group path is null', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000);
    const tile = makeTile('t1', { x: 100, y: 100, cwd: '/proj/a' });
    const groups = buildProjectGroups({ frames: [frame], tiles: [tile], projects: [] });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.drifted).toHaveLength(0);
  });

  test('tiles with no candidate path are not drifted even if path differs', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000, { path: '/proj/a' });
    const tile = makeTile('t1', { x: 100, y: 100, type: 'note' });
    const groups = buildProjectGroups({ frames: [frame], tiles: [tile], projects: ['/proj/a'] });
    const group = groups.find((g) => g.frameId === 'f1')!;
    expect(group.drifted).toHaveLength(0);
  });
});

describe('buildProjectGroups - outside routing', () => {
  test('non-frame tile matching frame group path goes to frame outside, not a path group', () => {
    const frame = makeFrame('f1', 0, 0, 200, 200);
    const projects = ['/proj/a'];
    const inFrame = makeTile('t1', { x: 50, y: 50, width: 20, height: 20, cwd: '/proj/a' });
    const outside = makeTile('t2', { x: 500, y: 500, cwd: '/proj/a' });

    const groups = buildProjectGroups({ frames: [frame], tiles: [inFrame, outside], projects });
    const frameGroup = groups.find((g) => g.frameId === 'f1')!;
    const pathGroup = groups.find((g) => g.kind === 'path');

    expect(frameGroup.outside).toContain(outside);
    expect(pathGroup).toBeUndefined();
  });

  test('non-frame tile with no matching project goes to path group', () => {
    const projects = ['/proj/a'];
    const tile = makeTile('t1', { x: 0, y: 0, cwd: '/proj/a' });
    const groups = buildProjectGroups({ frames: [], tiles: [tile], projects });
    const pathGroup = groups.find((g) => g.kind === 'path' && g.path === '/proj/a');
    expect(pathGroup).toBeDefined();
    expect(pathGroup!.tiles).toContain(tile);
  });

  test('non-frame tile with no tileProject result goes to loose group', () => {
    const tile = makeTile('t1', { x: 0, y: 0 });
    const groups = buildProjectGroups({ frames: [], tiles: [tile], projects: [] });
    const looseGroup = groups.find((g) => g.kind === 'loose');
    expect(looseGroup).toBeDefined();
    expect(looseGroup!.tiles).toContain(tile);
  });

  test('loose group is omitted when empty', () => {
    const groups = buildProjectGroups({ frames: [], tiles: [], projects: [] });
    expect(groups.find((g) => g.kind === 'loose')).toBeUndefined();
  });
});

describe('buildProjectGroups - empty project groups', () => {
  test('emits empty path group for project with no tiles and no matching frame path', () => {
    const groups = buildProjectGroups({ frames: [], tiles: [], projects: ['/proj/a'] });
    const pg = groups.find((g) => g.path === '/proj/a');
    expect(pg).toBeDefined();
    expect(pg!.tiles).toHaveLength(0);
  });

  test('does not emit empty path group for project already covered by frame path', () => {
    const frame = makeFrame('f1', 0, 0, 200, 200, { path: '/proj/a' });
    const groups = buildProjectGroups({ frames: [frame], tiles: [], projects: ['/proj/a'] });
    const pathGroups = groups.filter((g) => g.kind === 'path');
    expect(pathGroups).toHaveLength(0);
  });

  test('does not emit empty path group for a project whose tiles sit in another project frame', () => {
    const frame = makeFrame('f1', 0, 0, 1000, 1000, { path: '/game/client' });
    const tiles = [makeTile('c', { x: 10, y: 10, cwd: '/game/client' }), makeTile('s', { x: 10, y: 400, cwd: '/game/server' })];
    const groups = buildProjectGroups({ frames: [frame], tiles, projects: ['/game/client', '/game/server'] });
    expect(groups.filter((g) => g.kind === 'path')).toHaveLength(0);
    expect(groups[0]!.tiles.map((t) => t.id)).toEqual(['c', 's']);
  });
});

describe('buildProjectGroups - ordering', () => {
  test('frame groups come first in frames order', () => {
    const f1 = makeFrame('f1', 0, 0, 100, 100);
    const f2 = makeFrame('f2', 200, 0, 100, 100);
    const groups = buildProjectGroups({ frames: [f1, f2], tiles: [], projects: [] });
    expect(groups[0]?.frameId).toBe('f1');
    expect(groups[1]?.frameId).toBe('f2');
  });

  test('path groups follow projects order', () => {
    const projects = ['/proj/a', '/proj/b'];
    const tiles = [
      makeTile('t1', { x: 0, y: 0, cwd: '/proj/b' }),
      makeTile('t2', { x: 0, y: 0, cwd: '/proj/a' })
    ];
    const groups = buildProjectGroups({ frames: [], tiles, projects });
    const pathGroups = groups.filter((g) => g.kind === 'path');
    expect(pathGroups[0]?.path).toBe('/proj/a');
    expect(pathGroups[1]?.path).toBe('/proj/b');
  });

  test('loose group is last', () => {
    const projects = ['/proj/a'];
    const tiles = [
      makeTile('t1', { x: 0, y: 0, cwd: '/proj/a' }),
      makeTile('t2', { x: 0, y: 0 })
    ];
    const groups = buildProjectGroups({ frames: [], tiles, projects });
    expect(groups[groups.length - 1]?.kind).toBe('loose');
  });

  test('path group keys use normalized path', () => {
    const tiles = [makeTile('t1', { x: 0, y: 0, cwd: '/proj/a' })];
    const groups = buildProjectGroups({ frames: [], tiles, projects: ['/proj/a'] });
    const pg = groups.find((g) => g.kind === 'path')!;
    expect(pg.key).toBe('path:/proj/a');
  });
});

describe('tileOwnerResolver', () => {
  test('gives a pathless note inside a project frame to that frame project', () => {
    const frame: Frame = { id: 'f1', x: 0, y: 0, width: 800, height: 600, title: 'B', color: '#6b7280', path: '/b/bar' };
    const note = makeTile('n1', { x: 20, y: 20, type: 'note' });
    expect(tileOwnerResolver({ frames: [frame], tiles: [note], projects: ['/a/foo', '/b/bar'] })(note)).toBe('/b/bar');
  });

  test('lets the frame win over a terminal path, like the canvas groups do', () => {
    const frame: Frame = { id: 'f1', x: 0, y: 0, width: 800, height: 600, title: 'B', color: '#6b7280', path: '/b/bar' };
    const term = makeTile('t1', { x: 20, y: 20, cwd: '/a/foo' });
    expect(tileOwnerResolver({ frames: [frame], tiles: [term], projects: ['/a/foo', '/b/bar'] })(term)).toBe('/b/bar');
  });

  test('falls back to the tile path outside frames', () => {
    const term = makeTile('t1', { x: 2000, y: 2000, cwd: '/a/foo' });
    expect(tileOwnerResolver({ frames: [], tiles: [term], projects: ['/a/foo', '/b/bar'] })(term)).toBe('/a/foo');
  });
});

describe('projectsInUse', () => {
  test('keeps projects backed by a frame or a tile and drops the rest', () => {
    const frame = makeFrame('f1', 0, 0, 200, 200, { path: '/proj/framed' });
    const tiles = [makeTile('t', { x: 500, y: 500, cwd: '/proj/tiled' })];
    const projects = ['/proj/framed', '/proj/tiled', '/proj/empty'];
    expect(projectsInUse({ frames: [frame], tiles, projects })).toEqual(['/proj/framed', '/proj/tiled']);
  });
});
