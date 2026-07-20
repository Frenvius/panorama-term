import { expect, test } from 'bun:test';

import { toggleTaskAt, stripHiddenMarks } from '~/usecase/util/markdownLivePreview';

test('toggleTaskAt flips checkbox both ways', () => {
  expect(toggleTaskAt('[ ]')).toBe('[x]');
  expect(toggleTaskAt('[x]')).toBe('[ ]');
  expect(toggleTaskAt('[X]')).toBe('[ ]');
});

test('stripHiddenMarks drops rendered block marks, keeps indent and content', () => {
  expect(stripHiddenMarks('- [ ] Buy milk')).toBe('Buy milk');
  expect(stripHiddenMarks('  - [x] Done')).toBe('  Done');
  expect(stripHiddenMarks('## Heading')).toBe('Heading');
  expect(stripHiddenMarks('> quote')).toBe('quote');
  expect(stripHiddenMarks('- plain bullet')).toBe('- plain bullet');
  expect(stripHiddenMarks('a\n- [ ] b')).toBe('a\nb');
});
