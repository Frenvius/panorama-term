import { expect, test } from 'bun:test';

import { orderProjectChildren } from '~/usecase/util/projectGroups';

test('keeps saved children, drops missing ids and appends new children in natural order', () => {
  const children = ['a', 'b', 'c', 'd'].map((id) => ({ id }));
  expect(orderProjectChildren(children, ['c', 'removed', 'a', 'c']).map((child) => child.id)).toEqual(['c', 'a', 'b', 'd']);
  expect(orderProjectChildren(children, [])).toEqual(children);
});
