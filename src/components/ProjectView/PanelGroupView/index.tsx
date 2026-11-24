import type { PanelToolbarHandlers } from './PanelToolbar';
import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { AgentType } from '~/components/Terminal/AgentBar/parse';
import type { TerminalRestart } from '~/domain/interfaces/pty.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { DropZone } from '~/domain/interfaces/projectView.interface';
import type { PanelGroupViewState, PanelGroupViewHandlers } from '../types';

import React from 'react';
import {
  X,
  Moon,
  Plus,
  Power,
  Rows2,
  Pencil,
  Columns2,
  CopyPlus,
  FileCode,
  Maximize2,
  Minimize2,
  FolderOpen,
  GitCompare,
  StickyNote,
  ClipboardCopy,
  SquareKanban,
  SquareTerminal
} from 'lucide-react';

import PanelToolbar from './PanelToolbar';
import styles from './styles.module.scss';
import FileEditor from '~/components/FileEditor';
import OrchBoard from '~/components/Canvas/OrchBoard';
import DiffViewer from '~/components/DiffViewer';
import { tileLabel } from '~/usecase/util/title';
import { noteTabs } from '~/usecase/util/noteTabs';
import NoteTile from '~/components/Canvas/NoteTile';
import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { revealPath } from '~/adapter/shell/shell.client';
import { parseFrontField } from '~/usecase/util/noteMeta';
import { reportAgent } from '~/usecase/util/tileActivity';
import { registerTileRestart } from '~/usecase/util/tileRestart';
import ContextMenu from '~/components/commons/ContextMenu';
import CrashNotice from '~/components/commons/CrashNotice';
import GridTerminal from '~/components/Terminal/GridTerminal';
import DormantTerminal from '~/components/Terminal/DormantTerminal';
import ErrorBoundary from '~/components/commons/ErrorBoundary';
import { isDirty, subscribeDirty } from '~/usecase/util/dirtyFiles';
import { fitCells, TERM_ROW_PX } from '~/usecase/util/termGrid';
import { useTermCellWidth } from '~/usecase/hooks/useTermCellWidth';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { PiLogo, KimiLogo, CodexLogo, OpenCodeLogo, GenericAgentLogo, AntigravityLogo } from '~/components/commons/AgentIcons';

export interface PanelGroupViewProps {
  state: PanelGroupViewState;
  handlers: PanelGroupViewHandlers;
}

const EMBED_MODE = { embedded: true };
const DEFAULT_RESTART: TerminalRestart = { key: 0 };

const panelCrash = (retry: () => void) => <CrashNotice title="This panel hit an error" action="Reload panel" onAction={retry} />;

const computeDropZone = (rect: DOMRect, clientX: number, clientY: number): DropZone => {
  const rx = rect.width > 0 ? (clientX - rect.left) / rect.width : 0.5;
  const ry = rect.height > 0 ? (clientY - rect.top) / rect.height : 0.5;
  if (rx < 0.25) return 'left';
  if (rx > 0.75) return 'right';
  if (ry < 0.25) return 'top';
  if (ry > 0.75) return 'bottom';
  return 'center';
};

const renderAgentIcon = (agentType: AgentType) => {
  if (agentType === 'antigravity') return <AntigravityLogo size={12} />;
  if (agentType === 'codex') return <CodexLogo size={12} />;
  if (agentType === 'opencode') return <OpenCodeLogo size={12} />;
  if (agentType === 'pi') return <PiLogo size={12} />;
  if (agentType === 'kimi') return <KimiLogo size={12} />;
  if (agentType === 'generic') return <GenericAgentLogo size={12} />;
  return <ClaudeLogo size={12} />;
};

const renderTileTypeIcon = (tile: Tile, agentType: AgentType | null) => {
  if (tile.type === 'term' && agentType) return renderAgentIcon(agentType);
  if (tile.type === 'editor') return <FileCode size={13} strokeWidth={1.75} />;
  if (tile.type === 'code') return <GitCompare size={13} strokeWidth={1.75} />;
  if (tile.type === 'note') return <StickyNote size={13} strokeWidth={1.75} />;
  if (tile.type === 'orch') return <SquareKanban size={13} strokeWidth={1.75} />;
  return <SquareTerminal size={13} strokeWidth={1.75} />;
};

const toFullPath = (root: string, filePath: string): string => {
  if (/^(?:[A-Za-z]:[\\/]|\/)/.test(filePath)) return filePath;
  const normRoot = root.replace(/[\\/]+$/, '');
  const normRel = filePath.replace(/^[\\/]+/, '');
  return normRoot ? `${normRoot}/${normRel}` : filePath;
};

const PanelGroupView = (props: PanelGroupViewProps) => {
  const { state, handlers } = props;
  const { group, tileMap, isFocused, isMaximized, canCloseGroup, alerts, agents, draggingTab, wsId } = state;
  const cellW = useTermCellWidth();
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const renameInputRef = React.useRef<HTMLInputElement>(null);

  const [bodySize, setBodySize] = React.useState({ w: 640, h: 420 });
  const [dropZone, setDropZone] = React.useState<DropZone | null>(null);
  const [dragOverTabIndex, setDragOverTabIndex] = React.useState<number | null>(null);
  const [renamingTileId, setRenamingTileId] = React.useState<string | null>(null);
  const [renameDraft, setRenameDraft] = React.useState('');
  const [tabContextMenu, setTabContextMenu] = React.useState<{ tile: Tile; x: number; y: number } | null>(null);
  const [dirtyTick, setDirtyTick] = React.useState(0);

  const [agentTypes, setAgentTypes] = React.useState<Record<string, AgentType | null>>({});
  const [diffs, setDiffs] = React.useState<Record<string, { a: number; r: number } | null>>({});
  const [restarts, setRestarts] = React.useState<Record<string, TerminalRestart>>({});

  React.useEffect(() => {
    return subscribeDirty(() => setDirtyTick((n) => n + 1));
  }, []);

  React.useEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      setBodySize({ w: rect.width, h: rect.height });
    }
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      if (width > 0 && height > 0) {
        setBodySize({ w: width, h: height });
      }
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  React.useEffect(() => {
    if (renamingTileId) renameInputRef.current?.select();
  }, [renamingTileId]);

  React.useEffect(() => {
    if (!draggingTab) {
      setDropZone(null);
      setDragOverTabIndex(null);
    }
  }, [draggingTab]);

  const groupTiles = React.useMemo(() => {
    const list: Tile[] = [];
    for (const id of group.tileIds) {
      const tile = tileMap.get(id);
      if (tile) list.push(tile);
    }
    return list;
  }, [group.tileIds, tileMap]);

  const activeTile = React.useMemo(() => {
    if (group.activeTileId) {
      const found = tileMap.get(group.activeTileId);
      if (found && group.tileIds.includes(found.id)) return found;
    }
    return groupTiles[0] ?? null;
  }, [group.activeTileId, group.tileIds, tileMap, groupTiles]);

  const termTiles = React.useMemo(() => groupTiles.filter((t) => t.type === 'term' && !t.dormant), [groupTiles]);

  const termCols = Math.max(20, fitCells(bodySize.w, 8, cellW));
  const termRows = Math.max(2, fitCells(bodySize.h, 10, TERM_ROW_PX));

  const noteSections = activeTile?.type === 'note' ? noteTabs(activeTile.content) : [];
  const activeNoteTabName = activeTile?.type === 'note' ? parseFrontField(activeTile.content, 'tab') : '';
  const activeNoteTab = Math.max(
    0,
    noteSections.findIndex((s) => s.name === activeNoteTabName)
  );

  const focusGroup = () => {
    if (!isFocused) handlers.onFocusGroup(group.id);
  };

  const activateTab = (tileId: string) => () => {
    handlers.onActivateTile(tileId, group.id);
  };

  const closeTab = (tileId: string) => (e: React.MouseEvent) => {
    e.stopPropagation();
    handlers.onCloseTile(tileId);
  };

  const startRename = (tile: Tile) => {
    setRenameDraft(tile.userTitle || tileLabel(tile));
    setRenamingTileId(tile.id);
  };

  const onTabDoubleClick = (tile: Tile) => (e: React.MouseEvent) => {
    e.stopPropagation();
    startRename(tile);
  };

  const commitRename = () => {
    if (!renamingTileId) return;
    const targetId = renamingTileId;
    const trimmed = renameDraft.trim();
    setRenamingTileId(null);
    handlers.onRenameTile(targetId, trimmed);
  };

  const onRenameKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') commitRename();
    if (e.key === 'Escape') setRenamingTileId(null);
  };

  const onTabDragStart = (tileId: string) => (e: React.DragEvent) => {
    e.dataTransfer.effectAllowed = 'move';
    e.dataTransfer.setData('text/plain', tileId);
    handlers.onDragStartTab({ tileId, sourceGroupId: group.id });
  };

  const onTabDragEnd = () => {
    setDropZone(null);
    setDragOverTabIndex(null);
    handlers.onDragStartTab(null);
  };

  const onTabDragOver = (index: number) => (e: React.DragEvent) => {
    if (!draggingTab) return;
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'move';
    setDragOverTabIndex(index);
  };

  const onTabDrop = (index: number) => (e: React.DragEvent) => {
    if (!draggingTab) return;
    e.preventDefault();
    e.stopPropagation();
    setDragOverTabIndex(null);
    handlers.onDropTab({
      tileId: draggingTab.tileId,
      targetGroupId: group.id,
      zone: 'center',
      targetIndex: index
    });
    handlers.onDragStartTab(null);
  };

  const onTabBarDragOver = (e: React.DragEvent) => {
    if (!draggingTab) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
  };

  const onTabBarDrop = (e: React.DragEvent) => {
    if (!draggingTab) return;
    e.preventDefault();
    setDragOverTabIndex(null);
    handlers.onDropTab({
      tileId: draggingTab.tileId,
      targetGroupId: group.id,
      zone: 'center'
    });
    handlers.onDragStartTab(null);
  };

  const onBodyDragOver = (e: React.DragEvent) => {
    if (!draggingTab) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    const rect = e.currentTarget.getBoundingClientRect();
    const nextZone = computeDropZone(rect, e.clientX, e.clientY);
    if (nextZone !== dropZone) setDropZone(nextZone);
  };

  const onBodyDragLeave = (e: React.DragEvent) => {
    const related = e.relatedTarget as Node | null;
    if (related && e.currentTarget.contains(related)) return;
    setDropZone(null);
  };

  const onBodyDrop = (e: React.DragEvent) => {
    if (!draggingTab) return;
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    const zone = computeDropZone(rect, e.clientX, e.clientY);
    setDropZone(null);
    handlers.onDropTab({
      tileId: draggingTab.tileId,
      targetGroupId: group.id,
      zone
    });
    handlers.onDragStartTab(null);
  };

  const openTabContextMenu = (tile: Tile) => (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    handlers.onActivateTile(tile.id, group.id);
    setTabContextMenu({ tile, x: e.clientX, y: e.clientY });
  };

  const closeTabContextMenu = () => setTabContextMenu(null);

  const tabMenuItems: ContextMenuEntry[] = React.useMemo(() => {
    if (!tabContextMenu) return [];
    const { tile } = tabContextMenu;
    const targetPath = tile.filePath ? toFullPath(tile.cwd ?? '', tile.filePath) : (tile.runCwd ?? tile.cwd);
    const items: ContextMenuEntry[] = [
      {
        label: 'Split right',
        icon: <Columns2 size={15} strokeWidth={1.75} />,
        onSelect: () => handlers.onDropTab({ tileId: tile.id, targetGroupId: group.id, zone: 'right' })
      },
      {
        label: 'Split down',
        icon: <Rows2 size={15} strokeWidth={1.75} />,
        onSelect: () => handlers.onDropTab({ tileId: tile.id, targetGroupId: group.id, zone: 'bottom' })
      }
    ];
    if (tile.type === 'term') {
      items.push({
        label: 'Duplicate',
        icon: <CopyPlus size={15} strokeWidth={1.75} />,
        onSelect: () => handlers.onDuplicateTile(tile.id)
      });
    }
    items.push({
      label: 'Rename',
      icon: <Pencil size={15} strokeWidth={1.75} />,
      onSelect: () => startRename(tile)
    });
    if (targetPath) {
      items.push(
        {
          label: 'Copy path',
          icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
          onSelect: () => writeClipboard(targetPath)
        },
        {
          label: 'Reveal in explorer',
          icon: <FolderOpen size={15} strokeWidth={1.75} />,
          onSelect: () => revealPath(targetPath)
        }
      );
    }
    items.push('separator');
    if (tile.type === 'term') {
      items.push(
        {
          label: 'Stop',
          icon: <Moon size={15} strokeWidth={1.75} />,
          onSelect: () => handlers.onCloseTile(tile.id)
        },
        {
          label: 'Terminate',
          icon: <Power size={15} strokeWidth={1.75} />,
          danger: true,
          onSelect: () => handlers.onTerminateTile(tile.id)
        }
      );
      return items;
    }
    items.push({
      label: 'Close panel',
      icon: <X size={15} strokeWidth={1.75} />,
      danger: true,
      onSelect: () => handlers.onCloseTile(tile.id)
    });
    return items;
  }, [tabContextMenu, group.id, handlers]);

  const spawnTerminalInGroup = () => handlers.onNewTerminal(group.id);
  const spawnNoteInGroup = () => handlers.onNewNote(group.id);
  const splitHorizontal = () => handlers.onSplitGroup('horizontal', group.id, true);
  const splitVertical = () => handlers.onSplitGroup('vertical', group.id, true);
  const toggleMaximize = () => handlers.onToggleMaximize(group.id);
  const closeGroup = () => handlers.onCloseGroup(group.id);

  const onTermAgentActive = (tileId: string) => (agent: AgentType | null) => {
    setAgentTypes((prev) => (prev[tileId] === agent ? prev : { ...prev, [tileId]: agent }));
    const busy = agents.get(tileId) === 'busy';
    handlers.onAgentState(tileId, agent !== null, busy);
    reportAgent(tileId, agent, busy);
  };

  const onTermClaudeStatus = (tileId: string) => (status: string) => {
    const busy = status === 'busy';
    const currentAgent = agentTypes[tileId] ?? null;
    handlers.onAgentState(tileId, currentAgent !== null, busy);
    reportAgent(tileId, currentAgent, busy);
  };

  const onTermClaudeDiff = (tileId: string) => (a: number, r: number) => {
    const nextDiff = a || r ? { a, r } : null;
    setDiffs((prev) => ({ ...prev, [tileId]: nextDiff }));
  };

  const restartActiveTerm = (command?: string) => {
    if (!activeTile) return;
    const id = activeTile.id;
    setRestarts((prev) => ({
      ...prev,
      [id]: { key: (prev[id]?.key ?? 0) + 1, command }
    }));
  };

  const termIdsKey = termTiles.map((t) => t.id).join(',');

  React.useEffect(() => {
    const unsubs = termTiles.map((t) =>
      registerTileRestart(t.id, (command) =>
        setRestarts((prev) => ({ ...prev, [t.id]: { key: (prev[t.id]?.key ?? 0) + 1, command } }))
      )
    );
    return () => unsubs.forEach((off) => off());
  }, [termIdsKey]);

  const toggleActiveNoteRaw = () => {
    if (!activeTile) return;
    handlers.onPatchTile(activeTile.id, { renderOnly: !activeTile.renderOnly });
  };

  const toolbarHandlers: PanelToolbarHandlers = {
    onToggleRaw: toggleActiveNoteRaw,
    onRestart: restartActiveTerm,
    onPatchTile: handlers.onPatchTile,
    onCwd: handlers.onCwd,
    onOpenDiff: handlers.onOpenDiff,
    onOpenFile: handlers.onOpenFile,
    onOpenRunOutput: handlers.onOpenRunOutput
  };

  const onNoteChange = (tileId: string, content: string) => {
    handlers.onPatchTile(tileId, { content });
  };

  const onNoteActivate = (tileId: string) => {
    handlers.onActivateTile(tileId, group.id);
  };

  const openCodeTileInEditor = () => {
    if (!activeTile || !activeTile.filePath) return;
    const root = activeTile.cwd ?? '';
    handlers.onOpenFile(root, toFullPath(root, activeTile.filePath));
  };

  void dirtyTick;

  return (
    <div className={isFocused ? `${styles.panelGroup} ${styles.focused}` : styles.panelGroup} onPointerDown={focusGroup}>
      <div className={styles.tabBar} onDragOver={onTabBarDragOver} onDrop={onTabBarDrop}>
        <div className={styles.tabList} role="tablist">
          {groupTiles.map((tile, index) => {
            const isActive = tile.id === activeTile?.id;
            const alertKind = alerts.get(tile.id);
            const isBusy = agents.get(tile.id) === 'busy';
            const isFileDirty = tile.type === 'editor' && Boolean(tile.filePath && isDirty(tile.filePath));
            const agentType = agentTypes[tile.id] ?? null;
            const isDragTarget = dragOverTabIndex === index;

            return (
              <div
                key={tile.id}
                role="tab"
                aria-selected={isActive}
                draggable={renamingTileId !== tile.id}
                className={[styles.tab, isActive && styles.tabActive, isDragTarget && styles.tabDragOver]
                  .filter(Boolean)
                  .join(' ')}
                onClick={activateTab(tile.id)}
                onDoubleClick={onTabDoubleClick(tile)}
                onContextMenu={openTabContextMenu(tile)}
                onDragStart={onTabDragStart(tile.id)}
                onDragEnd={onTabDragEnd}
                onDragOver={onTabDragOver(index)}
                onDrop={onTabDrop(index)}
              >
                <span className={styles.tabIcon}>{renderTileTypeIcon(tile, agentType)}</span>

                {renamingTileId === tile.id ? (
                  <input
                    ref={renameInputRef}
                    className={styles.renameInput}
                    value={renameDraft}
                    onChange={(e) => setRenameDraft(e.target.value)}
                    onBlur={commitRename}
                    onKeyDown={onRenameKeyDown}
                    onClick={(e) => e.stopPropagation()}
                    onDoubleClick={(e) => e.stopPropagation()}
                    autoFocus
                  />
                ) : (
                  <span className={styles.tabTitle}>{tileLabel(tile)}</span>
                )}

                {isFileDirty && <span className={styles.dirtyDot} title="Unsaved changes" />}
                {!isFileDirty && isBusy && <span className={styles.busyDot} title="Busy" />}
                {!isFileDirty && !isBusy && alertKind && (
                  <span className={alertKind === 'finished' ? `${styles.alertDot} ${styles.alertDone}` : styles.alertDot} />
                )}

                <button type="button" className={styles.tabClose} onClick={closeTab(tile.id)} aria-label="Close tab">
                  <X size={11} strokeWidth={2} />
                </button>
              </div>
            );
          })}
        </div>

        <div className={styles.groupActions}>
          <button
            type="button"
            className={styles.groupBtn}
            onClick={spawnTerminalInGroup}
            aria-label="New terminal"
            data-tooltip="New terminal"
          >
            <Plus size={13} strokeWidth={2} />
          </button>
          <button
            type="button"
            className={styles.groupBtn}
            onClick={spawnNoteInGroup}
            aria-label="New note"
            data-tooltip="New note"
          >
            <StickyNote size={12} strokeWidth={2} />
          </button>
          <button
            type="button"
            className={styles.groupBtn}
            onClick={splitHorizontal}
            aria-label="Split right"
            data-tooltip="Split right"
          >
            <Columns2 size={13} strokeWidth={2} />
          </button>
          <button
            type="button"
            className={styles.groupBtn}
            onClick={splitVertical}
            aria-label="Split down"
            data-tooltip="Split down"
          >
            <Rows2 size={13} strokeWidth={2} />
          </button>
          <button
            type="button"
            className={styles.groupBtn}
            onClick={toggleMaximize}
            aria-label={isMaximized ? 'Restore pane' : 'Maximize pane'}
            data-tooltip={isMaximized ? 'Restore pane' : 'Maximize pane'}
          >
            {isMaximized ? <Minimize2 size={12} strokeWidth={2} /> : <Maximize2 size={12} strokeWidth={2} />}
          </button>
          {canCloseGroup && (
            <button
              type="button"
              className={`${styles.groupBtn} ${styles.groupCloseBtn}`}
              onClick={closeGroup}
              aria-label="Close group"
              data-tooltip="Close group"
            >
              <X size={13} strokeWidth={2} />
            </button>
          )}
        </div>
      </div>

      {activeTile && activeTile.type !== 'orch' && (
        <PanelToolbar
          tile={activeTile}
          wsId={wsId}
          noteTab={activeNoteTab}
          agentType={agentTypes[activeTile.id] ?? null}
          diff={diffs[activeTile.id] ?? null}
          handlers={toolbarHandlers}
        />
      )}

      <div ref={bodyRef} className={styles.body} onDragOver={onBodyDragOver} onDragLeave={onBodyDragLeave} onDrop={onBodyDrop}>
        <ErrorBoundary fallback={panelCrash}>
          {termTiles.map((termTile) => {
            const isTermActive = termTile.id === activeTile?.id;
            const runView = Boolean(termTile.runCwd);
            return (
              <div key={termTile.id} className={styles.termSlot} data-active={isTermActive}>
                <GridTerminal
                  sessionId={termTile.ptySessionId}
                  readOnly={runView}
                  cwd={runView ? termTile.runCwd : termTile.cwd}
                  onCwd={handlers.onCwd}
                  onOscTitle={handlers.onOscTitle}
                  onAgentActive={onTermAgentActive(termTile.id)}
                  onClaudeStatus={onTermClaudeStatus(termTile.id)}
                  onClaudeDiff={onTermClaudeDiff(termTile.id)}
                  restart={restarts[termTile.id] ?? DEFAULT_RESTART}
                  elevated={false}
                  active={isFocused && isTermActive}
                  visible={isTermActive}
                  tileId={termTile.id}
                  cols={termCols}
                  rows={termRows}
                />
              </div>
            );
          })}

          {activeTile?.type === 'term' && activeTile.dormant && (
            <div className={styles.contentSlot}>
              <DormantTerminal
                cwd={activeTile.cwd}
                onStart={activateTab(activeTile.id)}
              />
            </div>
          )}

          {activeTile?.type === 'editor' && (
            <div className={styles.contentSlot}>
              <FileEditor path={activeTile.filePath ?? ''} active={isFocused} />
            </div>
          )}

          {activeTile?.type === 'code' && (
            <div className={styles.contentSlot}>
              <DiffViewer
                root={activeTile.cwd ?? ''}
                file={activeTile.filePath ?? ''}
                mode={EMBED_MODE}
                handlers={{ onEditFile: openCodeTileInEditor }}
              />
            </div>
          )}

          {activeTile?.type === 'note' && (
            <div className={styles.contentSlot}>
              <NoteTile
                tile={activeTile}
                tab={activeNoteTab}
                wsId={wsId}
                active={isFocused}
                onChange={onNoteChange}
                onActivate={onNoteActivate}
                onEditor={handlers.onNoteEditor}
              />
            </div>
          )}

          {activeTile?.type === 'orch' && (
            <div className={styles.contentSlot}>
              <OrchBoard
                runId={activeTile.runId ?? ''}
                tileId={activeTile.id}
                cwd={activeTile.cwd ?? ''}
                onPatchOrchTile={(patch) => handlers.onPatchTile(activeTile.id, patch)}
              />
            </div>
          )}

          {!activeTile && (
            <div className={styles.emptyGroup}>
              <span className={styles.emptyLabel}>No open panels in this group</span>
              <div className={styles.emptyActions}>
                <button type="button" className={styles.emptyBtn} onClick={spawnTerminalInGroup}>
                  <SquareTerminal size={14} strokeWidth={1.75} />
                  <span>New terminal</span>
                </button>
                <button type="button" className={styles.emptyBtn} onClick={spawnNoteInGroup}>
                  <StickyNote size={14} strokeWidth={1.75} />
                  <span>New note</span>
                </button>
              </div>
            </div>
          )}
        </ErrorBoundary>

        {draggingTab && dropZone && <div className={styles.dropOverlay} data-zone={dropZone} />}
      </div>

      {tabContextMenu && (
        <ContextMenu x={tabContextMenu.x} y={tabContextMenu.y} items={tabMenuItems} onClose={closeTabContextMenu} />
      )}
    </div>
  );
};

export default PanelGroupView;
