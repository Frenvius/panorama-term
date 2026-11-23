import type { TabMeta } from '~/domain/interfaces/workspace.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import React from 'react';
import { X, Plus, Columns3, LayoutGrid } from 'lucide-react';

import styles from './styles.module.scss';
import ContextMenu from '~/components/commons/ContextMenu';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { getAlerts, subscribeAlerts } from '~/usecase/util/alerts';
import { clearTabAlerts } from '~/usecase/util/alerts';
import { workspaceService } from '~/usecase/service/workspace.service';
import { getTabScope, subscribeTabScope, visibleTabs, showWorkspacePrefix } from '~/usecase/util/tabScope';

interface TabContextState {
  x: number;
  y: number;
  tab: TabMeta & { workspaceId: string };
}

const TabsBar = () => {
  const { tabs, activeId, workspaces, activeTabId, switchTab, switchWorkspace, createTab, deleteTab, renameTab, setTabMode } = useWorkspace();
  const scope = React.useSyncExternalStore(subscribeTabScope, getTabScope);
  const [otherTabs, setOtherTabs] = React.useState<(TabMeta & { workspaceId: string })[]>([]);
  const [revision, setRevision] = React.useState(0);
  React.useEffect(() => {
    if (scope !== 'global') return;
    let alive = true;
    void Promise.all(workspaces.filter((ws) => ws.id !== activeId).map(async (ws) =>
      (await workspaceService.getTabs(ws.id)).tabs.map((tab) => ({ ...tab, workspaceId: ws.id }))
    )).then((result) => { if (alive) setOtherTabs(result.flat()); }).catch(console.error);
    return () => { alive = false; };
  }, [scope, workspaces, activeId, tabs, revision]);
  const allTabs = [...tabs.map((tab) => ({ ...tab, workspaceId: activeId ?? '' })), ...otherTabs.filter((tab) => tab.workspaceId !== activeId)];
  const shownTabs = visibleTabs(allTabs, activeId, scope);
  const showPrefix = showWorkspacePrefix(shownTabs);
  const refreshOtherTabs = () => setRevision((value) => value + 1);
  const changeMode = async (tab: TabMeta & { workspaceId: string }, mode: 'canvas' | 'project') => {
    if (tab.workspaceId === activeId) return setTabMode(tab.id, mode);
    await workspaceService.setTabMode(tab.workspaceId, tab.id, mode);
    refreshOtherTabs();
  };
  const closeTab = async (tab: TabMeta & { workspaceId: string }) => {
    if (tab.workspaceId === activeId) return deleteTab(tab.id);
    const result = await workspaceService.deleteTab(tab.workspaceId, tab.id);
    if (result.deleted) clearTabAlerts(tab.id);
    refreshOtherTabs();
  };
  const alerts = React.useSyncExternalStore(subscribeAlerts, getAlerts);
  const [addMenuOpen, setAddMenuOpen] = React.useState(false);
  const [ctxMenu, setCtxMenu] = React.useState<TabContextState | null>(null);
  const [renamingId, setRenamingId] = React.useState<string | null>(null);
  const [draft, setDraft] = React.useState('');
  const addWrapRef = React.useRef<HTMLDivElement>(null);
  const renameRef = React.useRef<HTMLInputElement>(null);

  const alerted = React.useMemo(() => new Set([...alerts.values()].map((alert) => alert.tabId)), [alerts]);

  React.useEffect(() => {
    if (!addMenuOpen) return;
    const onPointerDown = (e: PointerEvent) => {
      if (!addWrapRef.current?.contains(e.target as Node)) setAddMenuOpen(false);
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setAddMenuOpen(false);
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    document.addEventListener('keydown', onKeyDown, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
      document.removeEventListener('keydown', onKeyDown, true);
    };
  }, [addMenuOpen]);

  const toggleAddMenu = () => setAddMenuOpen((open) => !open);

  const handleCreateCanvasTab = () => {
    setAddMenuOpen(false);
    void createTab('canvas');
  };

  const handleCreateProjectTab = () => {
    setAddMenuOpen(false);
    void createTab('project');
  };

  const closeCtxMenu = () => setCtxMenu(null);

  const stopPropagation = (e: React.MouseEvent) => e.stopPropagation();

  const startRename = (tab: TabMeta) => {
    setDraft(tab.name);
    setRenamingId(tab.id);
  };

  const commitRename = () => {
    if (!renamingId) return;
    const next = draft.trim();
    const current = shownTabs.find((tab) => tab.id === renamingId);
    setRenamingId(null);
    if (next && current && next !== current.name) {
      if (current.workspaceId === activeId) void renameTab(renamingId, next);
      else void workspaceService.renameTab(current.workspaceId, renamingId, next).then(refreshOtherTabs);
    }
  };

  const onRenameKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') commitRename();
    if (e.key === 'Escape') setRenamingId(null);
  };

  React.useEffect(() => {
    if (renamingId) renameRef.current?.select();
  }, [renamingId]);

  const ctxItems: ContextMenuEntry[] = React.useMemo(() => {
    if (!ctxMenu) return [];
    const { tab } = ctxMenu;
    const mode = tab.mode ?? 'canvas';
    const isProject = mode === 'project';
    const nextMode = isProject ? 'canvas' : 'project';
    const items: ContextMenuEntry[] = [
      {
        label: isProject ? 'Switch to Canvas' : 'Switch to Project panels',
        icon: isProject ? <LayoutGrid size={14} /> : <Columns3 size={14} />,
        onSelect: () => void changeMode(tab, nextMode)
      },
      {
        label: 'Rename tab',
        onSelect: () => startRename(tab)
      },
      'separator',
      {
        label: 'New Canvas tab',
        icon: <LayoutGrid size={14} />,
        onSelect: () => void createTab('canvas')
      },
      {
        label: 'New Project tab',
        icon: <Columns3 size={14} />,
        onSelect: () => void createTab('project')
      }
    ];
    if (shownTabs.filter((item) => item.workspaceId === tab.workspaceId).length > 1) {
      items.push('separator', {
        label: 'Close tab',
        danger: true,
        onSelect: () => void closeTab(tab)
      });
    }
    return items;
  }, [ctxMenu, shownTabs, activeId, setTabMode, createTab, deleteTab]);

  return (
    <div data-no-drag className={styles.bar}>
      {shownTabs.map((tab) => {
        const mode = tab.mode ?? 'canvas';
        const workspace = workspaces.find((ws) => ws.id === tab.workspaceId);
        const select = () => void (tab.workspaceId === activeId ? switchTab(tab.id) : switchWorkspace(tab.workspaceId, tab.id));
        const rename = () => startRename(tab);
        const close = (e: React.MouseEvent) => {
          e.stopPropagation();
          void closeTab(tab);
        };
        const openContext = (e: React.MouseEvent) => {
          e.preventDefault();
          e.stopPropagation();
          setAddMenuOpen(false);
          setCtxMenu({ x: e.clientX, y: e.clientY, tab });
        };

        return (
          <div
            key={tab.id}
            className={styles.tab}
            onClick={select}
            onDoubleClick={rename}
            onContextMenu={openContext}
            data-active={tab.id === activeTabId}
          >
            {alerted.has(tab.id) && tab.id !== activeTabId && <span className={styles.dot} />}
            {showPrefix && workspace && <span className={styles.projectHint} title={workspace.name}>{workspace.name}</span>}
            <span className={styles.modeIcon}>
              {mode === 'project' ? <Columns3 size={12} /> : <LayoutGrid size={12} />}
            </span>
            {renamingId === tab.id ? (
              <input
                ref={renameRef}
                className={styles.renameInput}
                value={draft}
                aria-label="Tab name"
                onBlur={commitRename}
                onKeyDown={onRenameKey}
                onClick={stopPropagation}
                onDoubleClick={stopPropagation}
                onChange={(e) => setDraft(e.target.value)}
                autoFocus
              />
            ) : (
              <span className={styles.name}>{tab.name}</span>
            )}
            <button
              className={styles.close}
              onClick={close}
              aria-label="Close tab"
              data-hidden={shownTabs.filter((item) => item.workspaceId === tab.workspaceId).length <= 1}
            >
              <X size={13} strokeWidth={2} />
            </button>
          </div>
        );
      })}
      <div ref={addWrapRef} className={styles.addWrap}>
        <button
          className={styles.add}
          onClick={toggleAddMenu}
          aria-label="New tab"
          data-open={addMenuOpen}
        >
          <Plus size={16} strokeWidth={2} />
        </button>
        {addMenuOpen && (
          <div className={styles.popover}>
            <button className={styles.popoverOption} onClick={handleCreateCanvasTab}>
              <span className={styles.popoverIcon}>
                <LayoutGrid size={14} />
              </span>
              <span className={styles.popoverText}>
                <span className={styles.popoverTitle}>Canvas</span>
                <span className={styles.popoverDesc}>Infinite spatial canvas</span>
              </span>
            </button>
            <button className={styles.popoverOption} onClick={handleCreateProjectTab}>
              <span className={styles.popoverIcon}>
                <Columns3 size={14} />
              </span>
              <span className={styles.popoverText}>
                <span className={styles.popoverTitle}>Project panels</span>
                <span className={styles.popoverDesc}>Project sidebar & split panels</span>
              </span>
            </button>
          </div>
        )}
      </div>
      {ctxMenu && (
        <ContextMenu x={ctxMenu.x} y={ctxMenu.y} items={ctxItems} onClose={closeCtxMenu} />
      )}
    </div>
  );
};

export default TabsBar;
