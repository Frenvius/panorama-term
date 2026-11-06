import type { CommandBarItem } from '~/domain/interfaces/commandBar.interface';

import { expect, test } from 'bun:test';

import { matchScore, buildSections } from '~/usecase/util/commandBar';

const item = (over: Partial<CommandBarItem> & Pick<CommandBarItem, 'id' | 'kind' | 'label'>): CommandBarItem => ({
  run: () => {},
  ...over
});

const ITEMS: CommandBarItem[] = [
  item({ id: 't1', kind: 'tile', label: 'api server' }),
  item({ id: 't2', kind: 'tile', label: 'web', scope: 'Tab 2' }),
  item({ id: 'c1', kind: 'command', label: 'New terminal' }),
  item({ id: 's1', kind: 'setting', label: 'Terminal' }),
  item({ id: 'w1', kind: 'workspace', label: 'Personal' })
];

const ids = (query: string): string[][] => buildSections(ITEMS, query).map((s) => s.rows.map((r) => r.item.id));

test('substring match highlights the contiguous run', () => {
  expect(matchScore('New terminal', 'term')?.hits).toEqual([4, 5, 6, 7]);
});

test('subsequence match highlights each hit', () => {
  expect(matchScore('api server', 'asr')?.hits).toEqual([0, 4, 6]);
});

test('no match returns null', () => {
  expect(matchScore('web', 'xyz')).toBeNull();
});

test('empty query lists only tiles of the current tab', () => {
  const sections = buildSections(ITEMS, '');
  expect(sections.map((s) => s.kind)).toEqual(['recent']);
  expect(ids('')).toEqual([['t1']]);
});

test('typed query searches every kind and puts the best section first', () => {
  const sections = buildSections(ITEMS, 'term');
  expect(sections.map((s) => s.kind)).toEqual(['setting', 'command']);
});

test('scope is searchable but not highlighted', () => {
  const [section] = buildSections(ITEMS, 'tab 2');
  expect(section.rows[0].item.id).toBe('t2');
  expect(section.rows[0].hits).toEqual([]);
});

test('command prefix narrows to commands and settings', () => {
  expect(buildSections(ITEMS, '>').map((s) => s.kind)).toEqual(['command', 'setting']);
  expect(ids('> per')).toEqual([]);
});
