import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import { expect, test, describe } from 'bun:test';

import { frameLinks, isManualLink, linkKey } from '~/usecase/util/frameLinks';

const frame = (id: string, rect: Pick<Frame, 'x' | 'y' | 'width' | 'height'>): Frame => ({
  id,
  ...rect,
  title: id,
  color: '#000'
});

const tile = (id: string, type: Tile['type'], at: Pick<Tile, 'x' | 'y'>, opts?: Partial<Tile>): Tile => ({
  id,
  type,
  ...at,
  width: 100,
  height: 100,
  zIndex: 1,
  ...opts
});

describe('frameLinks', () => {
  test('note x term in same frame', () => {
    const f = frame('f1', { x: 0, y: 0, width: 500, height: 500 });
    const n = tile('n1', 'note', { x: 50, y: 50 });
    const t1 = tile('t1', 'term', { x: 200, y: 50 });
    const links = frameLinks([n, t1], [f]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ kind: 'note', a: 'n1', b: 't1' });
  });

  test('term x term pair in same frame, ids sorted', () => {
    const f = frame('f1', { x: 0, y: 0, width: 500, height: 500 });
    const t1 = tile('ta', 'term', { x: 50, y: 50 });
    const t2 = tile('tb', 'term', { x: 200, y: 50 });
    const links = frameLinks([t1, t2], [f]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ kind: 'term', a: 'ta', b: 'tb' });
  });

  test('term x term ids sorted ascending', () => {
    const f = frame('f1', { x: 0, y: 0, width: 500, height: 500 });
    const t1 = tile('tz', 'term', { x: 50, y: 50 });
    const t2 = tile('ta', 'term', { x: 200, y: 50 });
    const links = frameLinks([t1, t2], [f]);
    expect(links[0].a < links[0].b).toBe(true);
  });

  test('tile outside any frame is excluded', () => {
    const f = frame('f1', { x: 0, y: 0, width: 100, height: 100 });
    const n = tile('n1', 'note', { x: 50, y: 50 });
    const t1 = tile('t1', 'term', { x: 200, y: 200 });
    const links = frameLinks([n, t1], [f]);
    expect(links).toHaveLength(0);
  });

  test('nested frames - smallest wins', () => {
    const outer = frame('outer', { x: 0, y: 0, width: 600, height: 600 });
    const inner = frame('inner', { x: 50, y: 50, width: 300, height: 300 });
    const n = tile('n1', 'note', { x: 100, y: 100 });
    const t1 = tile('t1', 'term', { x: 200, y: 100 });
    const links = frameLinks([n, t1], [outer, inner]);
    expect(links).toHaveLength(1);
    expect(links[0]).toMatchObject({ kind: 'note', a: 'n1', b: 't1' });
  });

  test('tiles in different frames produce no cross-frame links', () => {
    const f1 = frame('f1', { x: 0, y: 0, width: 200, height: 200 });
    const f2 = frame('f2', { x: 300, y: 0, width: 200, height: 200 });
    const n = tile('n1', 'note', { x: 50, y: 50 });
    const t1 = tile('t1', 'term', { x: 350, y: 50 });
    const links = frameLinks([n, t1], [f1, f2]);
    expect(links).toHaveLength(0);
  });

  test('run term excluded (runCwd set)', () => {
    const f = frame('f1', { x: 0, y: 0, width: 500, height: 500 });
    const n = tile('n1', 'note', { x: 50, y: 50 });
    const run = tile('t1', 'term', { x: 200, y: 50 }, { runCwd: '/tmp' });
    const links = frameLinks([n, run], [f]);
    expect(links).toHaveLength(0);
  });

  test('run term excluded (runId set)', () => {
    const f = frame('f1', { x: 0, y: 0, width: 500, height: 500 });
    const t1 = tile('t1', 'term', { x: 50, y: 50 }, { runId: 'run1' });
    const t2 = tile('t2', 'term', { x: 200, y: 50 });
    const links = frameLinks([t1, t2], [f]);
    expect(links).toHaveLength(0);
  });

  test('orch worker excluded', () => {
    const f = frame('f1', { x: 0, y: 0, width: 500, height: 500 });
    const t1 = tile('t1', 'term', { x: 50, y: 50 }, { orchWorker: true });
    const t2 = tile('t2', 'term', { x: 200, y: 50 });
    const links = frameLinks([t1, t2], [f]);
    expect(links).toHaveLength(0);
  });

  test('multiple terms produce all pairs', () => {
    const f = frame('f1', { x: 0, y: 0, width: 600, height: 600 });
    const t1 = tile('t1', 'term', { x: 50, y: 50 });
    const t2 = tile('t2', 'term', { x: 200, y: 50 });
    const t3 = tile('t3', 'term', { x: 350, y: 50 });
    const links = frameLinks([t1, t2, t3], [f]);
    const termLinks = links.filter((l) => l.kind === 'term');
    expect(termLinks).toHaveLength(3);
  });
});

describe('isManualLink', () => {
  test('note link detected via linkedTo on note', () => {
    const n = tile('n1', 'note', { x: 0, y: 0 }, { linkedTo: ['t1'] });
    const t1 = tile('t1', 'term', { x: 100, y: 0 });
    expect(isManualLink([n, t1], { kind: 'note', a: 'n1', b: 't1' })).toBe(true);
  });

  test('note link not present', () => {
    const n = tile('n1', 'note', { x: 0, y: 0 }, { linkedTo: [] });
    const t1 = tile('t1', 'term', { x: 100, y: 0 });
    expect(isManualLink([n, t1], { kind: 'note', a: 'n1', b: 't1' })).toBe(false);
  });

  test('term link detected via linkedTo on a', () => {
    const t1 = tile('ta', 'term', { x: 0, y: 0 }, { linkedTo: ['tb'] });
    const t2 = tile('tb', 'term', { x: 100, y: 0 });
    expect(isManualLink([t1, t2], { kind: 'term', a: 'ta', b: 'tb' })).toBe(true);
  });

  test('term link detected via linkedTo on b', () => {
    const t1 = tile('ta', 'term', { x: 0, y: 0 });
    const t2 = tile('tb', 'term', { x: 100, y: 0 }, { linkedTo: ['ta'] });
    expect(isManualLink([t1, t2], { kind: 'term', a: 'ta', b: 'tb' })).toBe(true);
  });

  test('term link not present', () => {
    const t1 = tile('ta', 'term', { x: 0, y: 0 });
    const t2 = tile('tb', 'term', { x: 100, y: 0 });
    expect(isManualLink([t1, t2], { kind: 'term', a: 'ta', b: 'tb' })).toBe(false);
  });
});

describe('linkKey', () => {
  test('note key format', () => {
    expect(linkKey({ kind: 'note', a: 'n1', b: 't1' })).toBe('note:n1:t1');
  });

  test('term key format', () => {
    expect(linkKey({ kind: 'term', a: 'ta', b: 'tb' })).toBe('term:ta:tb');
  });
});
