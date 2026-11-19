import { expect, test, mock } from 'bun:test';

const db = new Map<string, unknown>();
const clone = <T>(v: T): T => structuredClone(v);
const tick = () => new Promise((resolve) => setTimeout(resolve, 1));

mock.module('~/adapter/store/store.client', () => ({
  storeList: async () => [],
  storeDelete: async (name: string) => void db.delete(name),
  storeRead: async (name: string) => {
    await tick();
    return db.has(name) ? clone(db.get(name)) : null;
  },
  storeWrite: async (name: string, value: unknown) => {
    await tick();
    db.set(name, clone(value));
  },
  storeWriteMany: async (entries: { name: string; value: unknown }[]) => {
    for (const e of entries) db.set(e.name, clone(e.value));
  }
}));

const { workspaceService } = await import('~/usecase/service/workspace.service');

test('concurrent tab save and tab switch keep both writes and never cross tabs', async () => {
  const { activeId } = await workspaceService.list();
  const wsId = activeId!;
  const tab2 = (await workspaceService.createTab(wsId))!;
  const { activeTabId: tab1 } = await workspaceService.getTabs(wsId);
  const tile = { id: 't1', type: 'note', x: 0, y: 0, width: 10, height: 10, zIndex: 1 };
  const state = { version: 1, tiles: [tile], viewport: { zoom: 1, centerX: 0, centerY: 0 } };

  await Promise.all([
    workspaceService.saveTabState(wsId, tab1!, state as never),
    workspaceService.setActiveTab(wsId, tab2.id)
  ]);

  expect((await workspaceService.getTabs(wsId)).activeTabId).toBe(tab2.id);
  expect((await workspaceService.loadTabState(wsId, tab1!))!.tiles).toHaveLength(1);
  expect((await workspaceService.loadTabState(wsId, tab2.id))!.tiles).toHaveLength(0);
});

test('invalidating a changed workspace reloads the new snapshot', async () => {
  const { activeId } = await workspaceService.list();
  const key = `workspaces/${activeId}.json`;
  const file = clone(db.get(key)) as { meta: { name: string } };
  file.meta.name = 'Changed remotely';
  db.set(key, file);
  expect((await workspaceService.list()).workspaces[0]!.name).not.toBe('Changed remotely');
  await workspaceService.invalidate([key]);
  expect((await workspaceService.list()).workspaces[0]!.name).toBe('Changed remotely');
});

test('resync invalidates every cached workspace', async () => {
  const { activeId } = await workspaceService.list();
  const key = `workspaces/${activeId}.json`;
  const file = clone(db.get(key)) as { meta: { name: string } };
  file.meta.name = 'After resync';
  db.set(key, file);
  await workspaceService.invalidate(['*']);
  expect((await workspaceService.list()).workspaces[0]!.name).toBe('After resync');
});

test('createTab with project mode and setTabMode persist across reloads', async () => {
  const { activeId } = await workspaceService.list();
  const wsId = activeId!;

  const projectTab = (await workspaceService.createTab(wsId, 'Project Tab', 'project'))!;
  expect(projectTab.mode).toBe('project');

  const tabsAfterCreate = (await workspaceService.getTabs(wsId)).tabs;
  expect(tabsAfterCreate.find((t) => t.id === projectTab.id)?.mode).toBe('project');

  await workspaceService.setTabMode(wsId, projectTab.id, 'canvas');
  expect((await workspaceService.getTabs(wsId)).tabs.find((t) => t.id === projectTab.id)?.mode).toBe('canvas');

  await workspaceService.setTabMode(wsId, projectTab.id, 'project');

  (workspaceService as unknown as { cachedFiles: Map<string, unknown> }).cachedFiles.clear();

  const reloadedTabs = (await workspaceService.getTabs(wsId)).tabs;
  expect(reloadedTabs.find((t) => t.id === projectTab.id)?.mode).toBe('project');
});
