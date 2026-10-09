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
