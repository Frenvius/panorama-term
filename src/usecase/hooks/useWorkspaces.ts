import type { TabMeta, TabMode, CanvasState, WorkspaceMeta } from '~/domain/interfaces/workspace.interface';

import React from 'react';
import { listen } from '@tauri-apps/api/event';
import { storeOrigin } from '~/adapter/store/store.client';
import { isRemote } from '~/adapter/transport';
import { remoteStatus } from '~/adapter/remote/remote.client';
import { createDeferredReload, listenPointerInteraction, shouldReload, type StoreChange } from '~/usecase/util/storeSync';

import { clearTabAlerts } from '~/usecase/util/alerts';
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

  const current = React.useRef({ activeId, activeTabId });
  current.current = { activeId, activeTabId };
  React.useEffect(() => {
    let alive = true;
    let queue = Promise.resolve();
    const deferred = createDeferredReload((names) => {
      queue = queue
        .then(async () => {
          await workspaceService.invalidate(names);
          const list = await workspaceService.list();
          if (!alive) return;
          setWorkspaces(list.workspaces);
          const { activeId: active, activeTabId: tab } = current.current;
          const target = active && list.workspaces.some((ws) => ws.id === active) ? active : list.activeId;
          if (target !== active) setActiveId(target);
          if (target && (target !== active || names.includes('*') || names.includes(`workspaces/${target}.json`))) {
            const result = await workspaceService.getTabs(target);
            const selected = tab && result.tabs.some((t) => t.id === tab) ? tab : result.activeTabId;
            const state = selected ? await workspaceService.loadTabState(target, selected) : null;
            if (!alive) return;
            setTabs(result.tabs);
            setActiveTabId(selected);
            setActiveState(state);
          }
        })
        .catch(console.error);
    });
    const stopPointer = listenPointerInteraction(
      window,
      () => deferred.start(),
      () => deferred.end()
    );
    const off = listen<StoreChange>('store:changed', ({ payload }) => {
      if (shouldReload(payload, storeOrigin)) deferred.change(payload.name);
    });
    const offResync = listen('resync', () => {
      deferred.change('*');
    });
    if (isRemote()) {
      void offResync.then(async () => {
        if (alive && (await remoteStatus()).status === 'connected') deferred.change('*');
      });
    }
    return () => {
      alive = false;
      stopPointer();
      void off.then((unlisten) => unlisten());
      void offResync.then((unlisten) => unlisten());
    };
  }, []);

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
      await Promise.all([workspaceService.setActiveTab(activeId, tabId), loadTabs(activeId, tabId)]);
    },
    [activeId, loadTabs]
  );

  const createTab = React.useCallback(
    async (mode: TabMode = 'canvas', name?: string) => {
      if (!activeId) return;
      const tab = await workspaceService.createTab(activeId, name, mode);
      if (!tab) return;
      await workspaceService.setActiveTab(activeId, tab.id);
      await loadTabs(activeId, tab.id);
    },
    [activeId, loadTabs]
  );

  const setTabMode = React.useCallback(
    async (tabId: string, mode: TabMode) => {
      if (!activeId) return;
      await workspaceService.setTabMode(activeId, tabId, mode);
      if (tabId === activeTabId) {
        await loadTabs(activeId, tabId);
      } else {
        await refreshTabsMeta(activeId);
      }
    },
    [activeId, activeTabId, loadTabs, refreshTabsMeta]
  );

  const deleteTab = React.useCallback(
    async (tabId: string) => {
      if (!activeId) return;
      const res = await workspaceService.deleteTab(activeId, tabId);
      if (!res.deleted) return;
      clearTabAlerts(tabId);
      await loadTabs(activeId, res.activeTabId ?? undefined);
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

  const saveActiveState = React.useCallback(
    (state: CanvasState) => {
      const live = current.current;
      if (live.activeId === activeId && live.activeTabId === activeTabId) setActiveState(state);
      if (activeId && activeTabId) void workspaceService.saveTabState(activeId, activeTabId, state);
    },
    [activeId, activeTabId]
  );

  const tabKey = `${activeId ?? 'none'}/${activeTabId ?? 'none'}`;
  const activeTabMode: TabMode = React.useMemo(
    () => tabs.find((t) => t.id === activeTabId)?.mode ?? 'canvas',
    [tabs, activeTabId]
  );

  return React.useMemo(
    () => ({
      tabs,
      tabKey,
      activeId,
      workspaces,
      activeState,
      activeTabId,
      activeTabMode,
      switchTab,
      createTab,
      deleteTab,
      renameTab,
      setTabMode,
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
      activeTabMode,
      switchTab,
      createTab,
      deleteTab,
      renameTab,
      setTabMode,
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
