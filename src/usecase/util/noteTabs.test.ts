import { expect, test } from 'bun:test';

import { joinTabs, nameTabs, splitTabs, withTabText } from '~/usecase/util/noteTabs';
import { withBody, setFrontField, applyFrontTitle, parseFrontField, stripFrontmatter } from '~/usecase/util/noteMeta';

test('plain body is a single unnamed tab', () => {
  expect(splitTabs('hello\nworld')).toEqual([{ name: '', text: 'hello\nworld' }]);
  expect(joinTabs([{ name: '', text: 'hello' }])).toBe('hello');
});

test('round-trips named tabs with empty and trailing-newline text', () => {
  const tabs = [
    { name: 'A', text: '' },
    { name: 'B', text: 'x\n' },
    { name: 'C', text: 'last' }
  ];
  expect(splitTabs(joinTabs(tabs))).toEqual(tabs);
});

test('text before the first marker becomes a leading tab', () => {
  expect(splitTabs('intro\n<!-- tab: B -->\nb')).toEqual([
    { name: '', text: 'intro' },
    { name: 'B', text: 'b' }
  ]);
});

test('nameTabs fills blanks with unused names', () => {
  const tabs = [
    { name: 'Tab 1', text: '' },
    { name: '', text: '' },
    { name: 'Tab 2', text: '' }
  ];
  expect(nameTabs(tabs).map((t) => t.name)).toEqual(['Tab 1', 'Tab 3', 'Tab 2']);
});

test('frontmatter fields survive title and body edits', () => {
  const raw = setFrontField(setFrontField('body', 'title', 'Todo'), 'tab', 'Ideas: v2');
  expect(parseFrontField(raw, 'tab')).toBe('Ideas: v2');
  const renamed = applyFrontTitle(withBody(raw, 'new'), 'Done');
  expect(parseFrontField(renamed, 'tab')).toBe('Ideas: v2');
  expect(parseFrontField(renamed, 'title')).toBe('Done');
  expect(stripFrontmatter(renamed)).toBe('new');
  expect(setFrontField(setFrontField(renamed, 'tab', ''), 'title', '')).toBe('new');
});

test('withTabText replaces only the target tab', () => {
  const body = joinTabs([{ name: 'A', text: 'a' }, { name: 'B', text: 'b' }]);
  expect(splitTabs(withTabText(body, 1, 'new'))).toEqual([
    { name: 'A', text: 'a' },
    { name: 'B', text: 'new' }
  ]);
});
