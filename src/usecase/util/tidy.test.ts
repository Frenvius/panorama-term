import { describe, it, expect } from 'bun:test';

import { buildTidySnapshot, sanitizeTidyPlan, computeTidyLayout } from './tidy';
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
    tileIds: ['t1', 't2', 't3', 't4']
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

  it('places colliding existing frame to the right without intersecting other frames', () => {
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

    expect(framePatches).toHaveLength(0);
    expect(newFrames).toHaveLength(1);

    const nf = newFrames[0];
    const f2 = frames[1];
    expect(noIntersect(nf.x, nf.y, nf.width, nf.height, f2.x, f2.y, f2.width, f2.height)).toBe(true);
    expect(noIntersect(nf.x, nf.y, nf.width, nf.height, frames[0].x, frames[0].y, frames[0].width, frames[0].height)).toBe(true);
  });
});
