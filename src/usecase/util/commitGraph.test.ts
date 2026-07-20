import type { LogRow } from '~/domain/interfaces/git.interface';

import { expect, test } from 'bun:test';

import { graphColor, buildCommitGraph } from '~/usecase/util/commitGraph';

const row = (short: string, parents: string[], refs = ''): LogRow => ({
  short,
  parents,
  refs,
  author: 'x',
  committer: 'x',
  date: '2026-01-01 00:00',
  message: short
});

test('linear history stays on one lane', () => {
  const graph = buildCommitGraph([row('a', ['b']), row('b', ['c']), row('c', [])]);

  expect(graph.map((g) => g.lane)).toEqual([0, 0, 0]);
  expect(graph.every((g) => g.width === 1)).toBe(true);
  expect(graph[2].edges.some((e) => e.kind === 'out')).toBe(false);
});

test('merge forks a lane and rejoins at the shared parent', () => {
  const graph = buildCommitGraph([row('a', ['b', 'c']), row('b', ['d']), row('c', ['d']), row('d', [])]);

  expect(graph.every((g) => g.width === 2)).toBe(true);

  expect(graph[0].lane).toBe(0);
  expect(graph[0].edges).toContainEqual({ fromLane: 0, toLane: 1, kind: 'out', color: 1 });

  expect(graph[1].lane).toBe(0);
  expect(graph[1].edges).toContainEqual({ fromLane: 1, toLane: 1, kind: 'through', color: 1 });

  expect(graph[2].lane).toBe(1);
  expect(graph[2].edges).toContainEqual({ fromLane: 0, toLane: 0, kind: 'through', color: 0 });

  expect(graph[3].lane).toBe(0);
  expect(graph[3].edges).toContainEqual({ fromLane: 1, toLane: 0, kind: 'in', color: 1 });
});

test('lane freed by a merge is reused by a later branch', () => {
  const graph = buildCommitGraph([
    row('a', ['b', 'c']),
    row('b', ['d']),
    row('c', ['d']),
    row('d', ['e', 'f']),
    row('e', ['g']),
    row('f', ['g'])
  ]);

  expect(graph[4].lane).toBe(0);
  expect(graph[5].lane).toBe(1);
  expect(graph.every((g) => g.width === 2)).toBe(true);
});

test('a branch keeps its color across repos via its ref name', () => {
  const here = buildCommitGraph([row('a', ['b'], 'HEAD -> main, origin/main')]);
  const there = buildCommitGraph([row('z', [], 'main')]);

  expect(here[0].color).toBe(there[0].color);
  expect(here[0].color).not.toBe(0);
});

test('tags and HEAD are skipped when picking the lane ref', () => {
  const tagged = buildCommitGraph([row('a', ['b'], 'tag: v1.0.0')]);

  expect(tagged[0].color).toBe(0);
});

test('generated colors vary only in hue', () => {
  const colors = [0, 1, 2, 3, 99].map(graphColor);

  expect(new Set(colors).size).toBe(5);
  for (const color of colors) expect(color).toMatch(/^hsl\(\d+(\.\d+)? 27\.1% 52%\)$/);
});
