import { expect, test } from 'bun:test';

import { fuzzyMatch } from '~/usecase/util/fuzzy';

test('empty query matches everything', () => {
  expect(fuzzyMatch('  ', 'main')).toEqual([]);
});

test('plain substring still matches', () => {
  expect(fuzzyMatch('login', 'feature/login')).toEqual([8, 9, 10, 11, 12]);
});

test('segment abbreviations match across separators', () => {
  expect(fuzzyMatch('fe/lo', 'feature/login')).toEqual([0, 1, 7, 8, 9]);
  expect(fuzzyMatch('flog', 'feature/login')).toEqual([0, 8, 9, 10]);
  expect(fuzzyMatch('fl', 'fix-login-bug')).toEqual([0, 4]);
});

test('camel humps count as word starts', () => {
  expect(fuzzyMatch('nb', 'newBranch')).toEqual([0, 3]);
});

test('scattered letters inside a word do not match', () => {
  expect(fuzzyMatch('mn', 'main')).toBeNull();
  expect(fuzzyMatch('ftr', 'feature/login')).toBeNull();
});

test('matching is case insensitive', () => {
  expect(fuzzyMatch('MAIN', 'main')).toEqual([0, 1, 2, 3]);
});

test('backtracks to a later start when the first one dead-ends', () => {
  expect(fuzzyMatch('log', 'old/login')).toEqual([4, 5, 6]);
});
