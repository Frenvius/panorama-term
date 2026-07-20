import React from 'react';

import type { TabMeta, CanvasState, WorkspaceMeta } from '~/domain/interfaces/workspace.interface';
import { workspaceService } from '~/usecase/service/workspace.service';
import { toStored, type RuntimeCanvas } from '~/usecase/util/workspaceCanvas';

export const useWorkspaces = () => {
  const [tabs, setTabs] = React.useState<TabMeta[]>([]);
  const [activeId, setActiveId] = React.useState<string | null>(null);
  const [activeTabId, setActiveTabId] = React.useState<string | null>(null);
  const [activeState, setActiveState] = React.useState<CanvasState | null>(null);
  const [workspaces, setWorkspaces] = React.useState<WorkspaceMeta[]>([]);

  const loadTabs = React.useCallback(async (wsId: string, tabId?: string) => {
    const res = await workspaceService.getTabs(wsId);
    const target = tabId ?? res.activeTabId;
    const state = target ? await workspaceService.loadTabState(wsId, target) : null;
    setTabs(res.tabs);
    setActiveTabId(target);
    setActiveState(state);
  }, []);

  const refresh = React.useCallback(async () => {
    const res = await workspaceService.list();
    setWorkspaces(res.workspaces);
    setActiveId(res.activeId);
    if (res.activeId) await loadTabs(res.activeId);
  }, [loadTabs]);

  React.useEffect(() => {
    void refresh();
  }, [refresh]);

  const refreshTabsMeta = React.useCallback(async (wsId: string) => {
    const res = await workspaceService.getTabs(wsId);
    setTabs(res.tabs);
  }, []);

  const switchWorkspace = React.useCallback(
    async (id: string) => {
      await workspaceService.setActive(id);
      setActiveId(id);
      await loadTabs(id);
    },
    [loadTabs]
  );

  const createWorkspace = React.useCallback(
    async (name?: string) => {
      const meta = await workspaceService.create(name);
      await workspaceService.setActive(meta.id);
      await refresh();
    },
    [refresh]
  );

  const deleteWorkspace = React.useCallback(
    async (id: string) => {
      const res = await workspaceService.delete(id);
      if (res.deleted) await refresh();
    },
    [refresh]
  );

  const renameWorkspace = React.useCallback(
    async (id: string, name: string) => {
      await workspaceService.rename(id, name);
      await refresh();
    },
    [refresh]
  );

  const setWorkspaceColor = React.useCallback(
    async (id: string, color: string) => {
      await workspaceService.setColor(id, color);
      await refresh();
    },
    [refresh]
  );

  const switchTab = React.useCallback(
    async (tabId: string) => {
      if (!activeId) return;
      await workspaceService.setActiveTab(activeId, tabId);
      await loadTabs(activeId, tabId);
    },
    [activeId, loadTabs]
  );

  const createTab = React.useCallback(async () => {
    if (!activeId) return;
    const tab = await workspaceService.createTab(activeId);
    if (!tab) return;
    await workspaceService.setActiveTab(activeId, tab.id);
    await loadTabs(activeId, tab.id);
  }, [activeId, loadTabs]);

  const deleteTab = React.useCallback(
    async (tabId: string) => {
      if (!activeId) return;
      const res = await workspaceService.deleteTab(activeId, tabId);
      if (res.deleted) await loadTabs(activeId, res.activeTabId ?? undefined);
    },
    [activeId, loadTabs]
  );

  const renameTab = React.useCallback(
    async (tabId: string, name: string) => {
      if (!activeId) return;
      await workspaceService.renameTab(activeId, tabId, name);
      await refreshTabsMeta(activeId);
    },
    [activeId, refreshTabsMeta]
  );

  const moveTileToTab = React.useCallback(
    async (tileId: string, current: RuntimeCanvas, targetTabId: string) => {
      if (!activeId || !activeTabId || targetTabId === activeTabId) return;
      const tile = current.tiles.find((t) => t.id === tileId);
      if (!tile) return;
      const source = toStored({ ...current, tiles: current.tiles.filter((t) => t.id !== tileId) });
      await workspaceService.saveTabState(activeId, activeTabId, source);
      const target = await workspaceService.loadTabState(activeId, targetTabId);
      if (!target) return;
      const zIndex = Math.max(0, ...target.tiles.map((t) => t.zIndex)) + 1;
      target.tiles = [...target.tiles, { ...tile, zIndex }];
      await workspaceService.saveTabState(activeId, targetTabId, target);
      await workspaceService.setActiveTab(activeId, targetTabId);
      await loadTabs(activeId, targetTabId);
    },
    [activeId, activeTabId, loadTabs]
  );

  const saveActiveState = React.useCallback((state: CanvasState) => {
    void workspaceService.saveActiveState(state);
  }, []);

  const tabKey = `${activeId ?? 'none'}/${activeTabId ?? 'none'}`;

  return React.useMemo(
    () => ({
      tabs,
      tabKey,
      activeId,
      workspaces,
      activeState,
      activeTabId,
      switchTab,
      createTab,
      deleteTab,
      renameTab,
      switchWorkspace,
      createWorkspace,
      deleteWorkspace,
      renameWorkspace,
      moveTileToTab,
      saveActiveState,
      setWorkspaceColor
    }),
    [
      tabs,
      tabKey,
      activeId,
      workspaces,
      activeState,
      activeTabId,
      switchTab,
      createTab,
      deleteTab,
      renameTab,
      switchWorkspace,
      createWorkspace,
      deleteWorkspace,
      renameWorkspace,
      moveTileToTab,
      saveActiveState,
      setWorkspaceColor
    ]
  );
};
