import type { TabMeta } from '~/domain/interfaces/workspace.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import React from 'react';
import { X, Plus, Columns3, LayoutGrid } from 'lucide-react';

import styles from './styles.module.scss';
import ContextMenu from '~/components/commons/ContextMenu';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { getAlerts, subscribeAlerts } from '~/usecase/util/alerts';

interface TabContextState {
  x: number;
  y: number;
  tab: TabMeta;
}

const TabsBar = () => {
  const { tabs, activeTabId, switchTab, createTab, deleteTab, renameTab, setTabMode } = useWorkspace();
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
    const current = tabs.find((tab) => tab.id === renamingId);
    setRenamingId(null);
    if (next && next !== current?.name) void renameTab(renamingId, next);
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
        onSelect: () => void setTabMode(tab.id, nextMode)
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
    if (tabs.length > 1) {
      items.push('separator', {
        label: 'Close tab',
        danger: true,
        onSelect: () => void deleteTab(tab.id)
      });
    }
    return items;
  }, [ctxMenu, tabs.length, setTabMode, createTab, deleteTab]);

  return (
    <div data-no-drag className={styles.bar}>
      {tabs.map((tab) => {
        const mode = tab.mode ?? 'canvas';
        const select = () => void switchTab(tab.id);
        const rename = () => startRename(tab);
        const close = (e: React.MouseEvent) => {
          e.stopPropagation();
          void deleteTab(tab.id);
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
              data-hidden={tabs.length <= 1}
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
