import { describe, it, expect } from 'bun:test';

import { pinPathlessNotes, buildTidySnapshot, sanitizeTidyPlan, computeTidyLayout } from './tidy';
import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { TidyPlan } from '~/domain/interfaces/orch.interface';

const makeTile = (id: string, overrides: Partial<Tile> = {}): Tile => ({
  id,
  type: 'term',
  x: 0,
  y: 0,
  width: 300,
  height: 200,
  zIndex: 1,
  ...overrides
});

const makeFrame = (id: string, overrides: Partial<Frame> = {}): Frame => ({
  id,
  x: 0,
  y: 0,
  width: 400,
  height: 300,
  title: 'Frame',
  color: '#6b7280',
  ...overrides
});

describe('buildTidySnapshot', () => {
  it('includes projects with order and display name', () => {
    const result = JSON.parse(buildTidySnapshot({ projects: ['/a/foo', '/b/bar'], frames: [], tiles: [] }));
    expect(result.projects).toHaveLength(2);
    expect(result.projects[0]).toMatchObject({ path: '/a/foo', name: 'foo', order: 0 });
    expect(result.projects[1]).toMatchObject({ path: '/b/bar', name: 'bar', order: 1 });
  });

  it('includes frames with id, title, path', () => {
    const frame = makeFrame('f1', { title: 'Work', path: '/a/foo' });
    const result = JSON.parse(buildTidySnapshot({ projects: [], frames: [frame], tiles: [] }));
    expect(result.frames[0]).toMatchObject({ id: 'f1', title: 'Work', path: '/a/foo' });
  });

  it('assigns frameId to tiles inside a frame', () => {
    const frame = makeFrame('f1', { x: 0, y: 0, width: 500, height: 400 });
    const inside = makeTile('t1', { x: 50, y: 50, width: 300, height: 200 });
    const outside = makeTile('t2', { x: 600, y: 0, width: 300, height: 200 });
    const result = JSON.parse(buildTidySnapshot({ projects: [], frames: [frame], tiles: [inside, outside] }));
    expect(result.tiles.find((t: { id: string }) => t.id === 't1').frameId).toBe('f1');
    expect(result.tiles.find((t: { id: string }) => t.id === 't2').frameId).toBeNull();
  });

  it('uses userTitle > oscTitle > autoTitle > type for tile title', () => {
    const t1 = makeTile('t1', { userTitle: 'my-title' });
    const t2 = makeTile('t2', { oscTitle: 'osc-title' });
    const t3 = makeTile('t3', { autoTitle: 'auto-title' });
    const t4 = makeTile('t4');
    const result = JSON.parse(buildTidySnapshot({ projects: [], frames: [], tiles: [t1, t2, t3, t4] }));
    expect(result.tiles[0].title).toBe('my-title');
    expect(result.tiles[1].title).toBe('osc-title');
    expect(result.tiles[2].title).toBe('auto-title');
    expect(result.tiles[3].title).toBe('term');
  });
});

describe('sanitizeTidyPlan', () => {
  const known = {
    projectPaths: ['/a/foo', '/b/bar', '/c/baz'],
    tiles: ['t1', 't2', 't3', 't4'].map((id) => makeTile(id)),
    frames: []
  };

  it('orders projects according to AI plan', () => {
    const plan: TidyPlan = { projectOrder: ['/c/baz', '/a/foo', '/b/bar'], frames: [] };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.projectOrder).toEqual(['/c/baz', '/a/foo', '/b/bar']);
  });

  it('drops unknown project paths', () => {
    const plan: TidyPlan = { projectOrder: ['/unknown/path', '/a/foo'], frames: [] };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.projectOrder).not.toContain('/unknown/path');
    expect(result.projectOrder).toContain('/a/foo');
  });

  it('appends projects omitted by the plan', () => {
    const plan: TidyPlan = { projectOrder: ['/a/foo'], frames: [] };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.projectOrder).toContain('/b/bar');
    expect(result.projectOrder).toContain('/c/baz');
    expect(result.projectOrder.indexOf('/a/foo')).toBe(0);
  });

  it('drops frames with unknown tile ids', () => {
    const plan: TidyPlan = {
      projectOrder: [],
      frames: [{ title: 'F', tileIds: ['unknown-id'] }]
    };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.frames).toHaveLength(0);
  });

  it('first assignment wins for tiles listed in multiple frames', () => {
    const plan: TidyPlan = {
      projectOrder: [],
      frames: [
        { title: 'First', tileIds: ['t1', 't2'] },
        { title: 'Second', tileIds: ['t1', 't3'] }
      ]
    };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.frames[0].tileIds).toContain('t1');
    expect(result.frames[1].tileIds).not.toContain('t1');
    expect(result.frames[1].tileIds).toContain('t3');
  });

  it('drops empty frames after filtering', () => {
    const plan: TidyPlan = {
      projectOrder: [],
      frames: [
        { title: 'Has tiles', tileIds: ['t1'] },
        { title: 'Empty', tileIds: [] }
      ]
    };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.frames).toHaveLength(1);
    expect(result.frames[0].title).toBe('Has tiles');
  });

  it('is case-insensitive for project path matching', () => {
    const plan: TidyPlan = { projectOrder: ['/A/FOO'], frames: [] };
    const result = sanitizeTidyPlan(plan, known);
    expect(result.projectOrder[0]).toBe('/a/foo');
  });
});

const noIntersect = (ax: number, ay: number, aw: number, ah: number, bx: number, by: number, bw: number, bh: number): boolean =>
  !(ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by);

describe('computeTidyLayout', () => {
  it('places tiles without overlap when widths differ', () => {
    const tiles = [
      makeTile('wide', { width: 400, height: 200 }),
      makeTile('narrow', { width: 100, height: 200 }),
      makeTile('med', { width: 250, height: 200 })
    ];
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: 'F', tileIds: ['wide', 'narrow', 'med'] }] };
    const { tileMoves } = computeTidyLayout(plan, tiles, []);

    const tileById = new Map(tiles.map((t) => [t.id, t]));
    const rects = tileMoves.map((m) => ({ x: m.x, y: m.y, w: tileById.get(m.id)!.width, h: tileById.get(m.id)!.height }));

    for (let i = 0; i < rects.length; i++) {
      for (let j = i + 1; j < rects.length; j++) {
        const a = rects[i];
        const b = rects[j];
        expect(noIntersect(a.x, a.y, a.w, a.h, b.x, b.y, b.w, b.h)).toBe(true);
      }
    }
  });

  it('moves a colliding existing frame to the right instead of duplicating it', () => {
    const tiles = [
      makeTile('t1', { width: 150, height: 100 }),
      makeTile('t2', { width: 150, height: 100 }),
      makeTile('t3', { width: 150, height: 100 }),
      makeTile('t4', { width: 150, height: 100 })
    ];
    const frames = [
      makeFrame('f1', { x: 0, y: 0, width: 200, height: 200 }),
      makeFrame('f2', { x: 350, y: 0, width: 200, height: 200 })
    ];
    const plan: TidyPlan = {
      projectOrder: [],
      frames: [{ title: 'F1', frameId: 'f1', tileIds: ['t1', 't2', 't3', 't4'] }]
    };
    const { framePatches, newFrames } = computeTidyLayout(plan, tiles, frames);

    expect(newFrames).toHaveLength(0);
    const moved = framePatches.find((p) => p.id === 'f1')!.patch as Frame;
    for (const f of frames) {
      expect(noIntersect(moved.x, moved.y, moved.width, moved.height, f.x, f.y, f.width, f.height)).toBe(true);
    }
  });

  it('keeps unplanned members inside a reused frame', () => {
    const frames = [makeFrame('f1', { x: 0, y: 0, width: 2000, height: 1000 })];
    const tiles = [makeTile('t1', { x: 40, y: 40 }), makeTile('keep', { x: 400, y: 40 })];
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: 'F', frameId: 'f1', tileIds: ['t1'] }] };
    const { tileMoves } = computeTidyLayout(plan, tiles, frames);
    expect(tileMoves.map((m) => m.id).sort()).toEqual(['keep', 't1']);
  });

  it('gives new frames the project path', () => {
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: '', path: '/a/foo', tileIds: ['t1'] }] };
    const { newFrames } = computeTidyLayout(plan, [makeTile('t1')], []);
    expect(newFrames[0]).toMatchObject({ path: '/a/foo', title: 'foo' });
  });

  it('completes path and title on frames the plan did not touch', () => {
    const frames = [makeFrame('f1', { title: '', width: 2000, height: 1000 })];
    const tiles = [makeTile('t1', { x: 40, y: 40, cwd: '/a/foo/src' })];
    const plan: TidyPlan = { projectOrder: [], frames: [] };
    const { framePatches } = computeTidyLayout(plan, tiles, frames, ['/a/foo']);
    expect(framePatches).toEqual([{ id: 'f1', patch: { path: '/a/foo', title: 'foo' } }]);
  });

  it('leaves complete project frames untouched', () => {
    const frames = [makeFrame('f1', { title: 'foo', path: '/a/foo', width: 2000, height: 1000 })];
    const tiles = [makeTile('t1', { x: 40, y: 40, cwd: '/a/foo' })];
    const { framePatches } = computeTidyLayout({ projectOrder: [], frames: [] }, tiles, frames, ['/a/foo']);
    expect(framePatches).toHaveLength(0);
  });
});

describe('sanitizeTidyPlan frame recognition', () => {
  const projects = ['/a/foo', '/b/bar'];
  const fooTile = makeTile('t1', { x: 40, y: 40, cwd: '/a/foo' });
  const barTile = makeTile('t2', { x: 3000, y: 40, cwd: '/b/bar' });

  it('reuses the existing project frame when the AI omits frameId', () => {
    const frames = [makeFrame('f1', { path: '/a/foo', width: 2000, height: 1000 })];
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: 'Foo', tileIds: ['t1'] }] };
    const result = sanitizeTidyPlan(plan, { projectPaths: projects, tiles: [fooTile], frames });
    expect(result.frames[0]).toMatchObject({ frameId: 'f1', path: '/a/foo' });
  });

  it('recognizes a pathless frame by its member tiles', () => {
    const frames = [makeFrame('f1', { width: 2000, height: 1000 })];
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: 'Foo', tileIds: ['t1'] }] };
    const result = sanitizeTidyPlan(plan, { projectPaths: projects, tiles: [fooTile], frames });
    expect(result.frames[0]).toMatchObject({ frameId: 'f1', path: '/a/foo' });
  });

  it('drops unknown frame ids and claims each frame once', () => {
    const frames = [makeFrame('f1', { path: '/a/foo', width: 2000, height: 1000 })];
    const plan: TidyPlan = {
      projectOrder: [],
      frames: [
        { title: 'A', frameId: 'f1', tileIds: ['t1'] },
        { title: 'B', frameId: 'f1', tileIds: ['t2'] },
        { title: 'C', frameId: 'ghost', tileIds: [] }
      ]
    };
    const result = sanitizeTidyPlan(plan, { projectPaths: projects, tiles: [fooTile, barTile], frames });
    expect(result.frames).toHaveLength(2);
    expect(result.frames[0].frameId).toBe('f1');
    expect(result.frames[1].frameId).toBeUndefined();
    expect(result.frames[1].path).toBe('/b/bar');
  });

  it('canonicalizes the AI project path', () => {
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: 'X', path: '/A/FOO/', tileIds: ['t2'] }] };
    const result = sanitizeTidyPlan(plan, { projectPaths: projects, tiles: [barTile], frames: [] });
    expect(result.frames[0].path).toBe('/a/foo');
  });
});

describe('tidy notes', () => {
  const note = (id: string, overrides: Partial<Tile> = {}) => makeTile(id, { type: 'note', ...overrides });

  it('leaves notes out of the snapshot', () => {
    const result = JSON.parse(buildTidySnapshot({ projects: [], frames: [], tiles: [makeTile('t1'), note('n1')] }));
    expect(result.tiles.map((t: { id: string }) => t.id)).toEqual(['t1']);
  });

  it('drops notes the plan tries to move', () => {
    const known = { projectPaths: [], tiles: [makeTile('t1'), note('n1')], frames: [] };
    const plan: TidyPlan = { projectOrder: [], frames: [{ title: 'F', tileIds: ['t1', 'n1'] }] };
    expect(sanitizeTidyPlan(plan, known).frames[0].tileIds).toEqual(['t1']);
  });

  it('keeps a note inside its reused frame', () => {
    const frame = makeFrame('f1', { x: 0, y: 0, width: 800, height: 600 });
    const tiles = [makeTile('t1', { x: 20, y: 20 }), note('n1', { x: 400, y: 20, width: 200, height: 200 })];
    const layout = computeTidyLayout({ projectOrder: [], frames: [{ title: 'F', frameId: 'f1', tileIds: ['t1'] }] }, tiles, [frame]);
    expect(layout.tileMoves.map((m) => m.id)).toContain('n1');
    expect(layout.newFrames).toHaveLength(0);
  });

  it('pins pathless notes to the project they show under before a reorder', () => {
    const tiles = [note('n1'), note('n2', { cwd: '/b/bar' }), makeTile('t1')];
    expect(pinPathlessNotes({ projects: ['/a/foo', '/b/bar'], frames: [], tiles })).toEqual([{ id: 'n1', workspacePath: '/a/foo' }]);
  });

  it('pins a pathless note to the project of the frame it sits in', () => {
    const frame = makeFrame('f1', { x: 0, y: 0, width: 800, height: 600, path: '/b/bar' });
    const tiles = [note('n1', { x: 20, y: 20, width: 200, height: 200 })];
    expect(pinPathlessNotes({ projects: ['/a/foo', '/b/bar'], frames: [frame], tiles })).toEqual([{ id: 'n1', workspacePath: '/b/bar' }]);
  });
});
