import { expect, test } from 'bun:test';
import { visibleTabs, showWorkspacePrefix } from '~/usecase/util/tabScope';

test('filters tabs by scope without changing the source', () => {
  const tabs = [{ workspaceId: 'a', id: '1' }, { workspaceId: 'b', id: '2' }];
  expect(visibleTabs(tabs, 'a', 'project')).toEqual([tabs[0]]);
  expect(visibleTabs(tabs, 'a', 'global')).toEqual(tabs);
  expect(visibleTabs(tabs, null, 'project')).toEqual([]);
  expect(tabs).toHaveLength(2);
});

test('shows workspace prefixes only when visible tabs span workspaces', () => {
  const tabs = [{ workspaceId: 'a' }, { workspaceId: 'a' }, { workspaceId: 'b' }];
  expect(showWorkspacePrefix([])).toBe(false);
  expect(showWorkspacePrefix(visibleTabs(tabs, 'a', 'project'))).toBe(false);
  expect(showWorkspacePrefix(tabs.slice(0, 2))).toBe(false);
  expect(showWorkspacePrefix(visibleTabs(tabs, 'a', 'global'))).toBe(true);
});
