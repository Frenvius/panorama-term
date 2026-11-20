import { isRemote } from '~/adapter/transport';
import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';
import type { ProjectsPanelProps, ProjectsPanelSignals, ProjectsPanelActions } from './types';

import React from 'react';
import {
  X,
  Plus,
  Clock,
  Folder,
  Search,
  Trash2,
  FileCode,
  GitBranch,
  FolderOpen,
  FolderPlus,
  GitCompare,
  StickyNote,
  ChevronDown,
  ChevronRight,
  ClipboardCopy,
  SquareDashed,
  SquareTerminal,
  TriangleAlert
} from 'lucide-react';

import styles from './styles.module.scss';
import ContextMenu from '~/components/commons/ContextMenu';
import { tileLabel } from '~/usecase/util/title';
import { getPref, setPref } from '~/usecase/util/prefs';
import { revealPath } from '~/adapter/shell/shell.client';
import { useRepoState } from '~/usecase/hooks/useRepoState';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { projectName } from '~/usecase/util/projectLayout';
import { tileStatus } from '~/usecase/util/tileStatus';
import { getTileActivity, subscribeTileActivity } from '~/usecase/util/tileActivity';

const SORT_KEY = 'panorama:projectsSort';
const COLLAPSED_KEY = 'panorama:projectsCollapsed';
const SORTS = ['projects', 'recent', 'attention'] as const;
type Sort = (typeof SORTS)[number];
const STATUS_RANK = { waiting: 0, working: 1, done: 2 } as const;
const TICK_MS = 30_000;

const savedSort = (): Sort => {
  const raw = getPref(SORT_KEY);
  return SORTS.includes(raw as Sort) ? (raw as Sort) : 'projects';
};

const savedCollapsed = (): Set<string> => {
  try {
    return new Set(JSON.parse(getPref(COLLAPSED_KEY) ?? '[]') as string[]);
  } catch {
    return new Set();
  }
};

const renderTileIcon = (tile: Tile) => {
  if (tile.type === 'note') return <StickyNote size={13} strokeWidth={1.75} />;
  if (tile.type === 'code') return <GitCompare size={13} strokeWidth={1.75} />;
  if (tile.type === 'editor') return <FileCode size={13} strokeWidth={1.75} />;
  return <SquareTerminal size={13} strokeWidth={1.75} />;
};

const stopPropagation = (e: React.MouseEvent) => e.stopPropagation();

interface TileRowProps {
  tile: Tile;
  isActive: boolean;
  isDrifted: boolean;
  isOutside: boolean;
  frameId: string | null;
  groupKey: string;
  signals: ProjectsPanelSignals;
  actions: ProjectsPanelActions;
  flatGroupTitle?: string;
  onContextMenu: (e: React.MouseEvent, tile: Tile) => void;
}

const TileRow = ({
  tile,
  isActive,
  isDrifted,
  isOutside,
  frameId,
  signals,
  actions,
  flatGroupTitle,
  onContextMenu
}: TileRowProps) => {
  const [renaming, setRenaming] = React.useState(false);
  const [draft, setDraft] = React.useState('');

  const tileAlert = signals.alerts.has(tile.id);
  const tileAgent = signals.agents.get(tile.id);
  const isDetached = Boolean(signals.detached?.has(tile.id));
  const dotKind: 'alert' | 'busy' | 'idle' | 'detached' | null = tileAlert
    ? 'alert'
    : tileAgent === 'busy'
      ? 'busy'
      : tileAgent === 'idle'
        ? 'idle'
        : isDetached
          ? 'detached'
          : null;

  const cwdBase = tile.cwd ? tile.cwd.split(/[\\/]/).pop() ?? '' : '';

  const handleActivate = () => actions.onActivateTile(tile);

  const handleStartRename = () => {
    setDraft(tile.userTitle || tileLabel(tile));
    setRenaming(true);
  };

  const commitRename = () => {
    if (renaming) actions.onRenameTile(tile.id, draft.trim());
    setRenaming(false);
  };

  const handleRenameChange = (e: React.ChangeEvent<HTMLInputElement>) => setDraft(e.target.value);

  const handleRenameKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') commitRename();
    if (e.key === 'Escape') setRenaming(false);
  };

  const handleClose = (e: React.MouseEvent) => {
    e.stopPropagation();
    actions.onCloseTile(tile.id);
  };

  const handleMoveIntoFrame = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (frameId) actions.onMoveTileIntoFrame(tile, frameId);
  };

  const handleContextMenu = (e: React.MouseEvent) => onContextMenu(e, tile);

  return (
    <div
      onClick={handleActivate}
      className={styles.tileRow}
      onDoubleClick={handleStartRename}
      onContextMenu={handleContextMenu}
      data-active={isActive || undefined}
      data-outside={isOutside || undefined}
    >
      <div className={styles.tileMain}>
        <span className={styles.tileIcon}>{renderTileIcon(tile)}</span>
        {renaming ? (
          <input
            autoFocus
            value={draft}
            onBlur={commitRename}
            onClick={stopPropagation}
            onKeyDown={handleRenameKey}
            onChange={handleRenameChange}
            className={styles.renameInput}
          />
        ) : (
          <span className={styles.tileTitle}>{tileLabel(tile)}</span>
        )}
        {isDrifted && cwdBase && (
          <span
            className={styles.driftChip}
            data-tooltip="Working directory is outside this project"
            data-tooltip-place="right"
          >
            {cwdBase}
          </span>
        )}
        {isOutside && frameId && (
          <button
            type="button"
            onClick={handleMoveIntoFrame}
            className={styles.warningBtn}
            aria-label="Outside frame. Click to move it in"
            data-tooltip="Outside frame. Click to move it in"
            data-tooltip-place="right"
          >
            <TriangleAlert size={11} strokeWidth={1.75} />
          </button>
        )}
        {flatGroupTitle && <span className={styles.flatGroupLabel}>{flatGroupTitle}</span>}
      </div>
      <div className={styles.tileRight}>
        {dotKind && (
          <span
            data-kind={dotKind}
            className={styles.statusDot}
            data-tooltip={isDetached ? 'Running in background' : undefined}
            data-tooltip-place="right"
          />
        )}
        <button type="button" onClick={handleClose} aria-label="Close tile" className={styles.closeTileBtn}>
          <X size={12} strokeWidth={2} />
        </button>
      </div>
    </div>
  );
};

interface GroupRowProps {
  group: ProjectGroup;
  query: string;
  isOpen: boolean;
  isActive: boolean;
  activeTileId: string | null;
  signals: ProjectsPanelSignals;
  actions: ProjectsPanelActions;
  onToggle: (key: string) => void;
  onGroupContextMenu: (e: React.MouseEvent, group: ProjectGroup) => void;
  onTileContextMenu: (e: React.MouseEvent, tile: Tile) => void;
}

const GroupRow = ({
  group,
  query,
  isOpen,
  isActive,
  activeTileId,
  signals,
  actions,
  onToggle,
  onGroupContextMenu,
  onTileContextMenu
}: GroupRowProps) => {
  const repo = useRepoState(group.path ?? undefined);
  const [renamingFrame, setRenamingFrame] = React.useState(false);
  const [frameDraft, setFrameDraft] = React.useState('');

  const branch = repo?.head ?? null;
  const allTiles = [...group.tiles, ...group.outside];

  const visibleTiles = React.useMemo(() => {
    if (!query) return group.tiles;
    const groupMatches =
      group.title.toLowerCase().includes(query) ||
      (group.path?.toLowerCase().includes(query) ?? false) ||
      (branch?.toLowerCase().includes(query) ?? false);
    if (groupMatches) return group.tiles;
    return group.tiles.filter((t) => tileLabel(t).toLowerCase().includes(query));
  }, [group.tiles, group.title, group.path, query, branch]);

  const visibleOutside = React.useMemo(() => {
    if (!query) return group.outside;
    const groupMatches = group.title.toLowerCase().includes(query);
    if (groupMatches) return group.outside;
    return group.outside.filter((t) => tileLabel(t).toLowerCase().includes(query));
  }, [group.outside, group.title, query]);

  const hasVisibleTiles = visibleTiles.length > 0 || visibleOutside.length > 0;

  if (query && !hasVisibleTiles && !group.title.toLowerCase().includes(query) && !(group.path?.toLowerCase().includes(query) ?? false)) {
    return null;
  }

  const hasAlert = allTiles.some((t) => signals.alerts.has(t.id));
  const hasBusy = allTiles.some((t) => signals.agents.get(t.id) === 'busy');
  const groupSignal: 'alert' | 'busy' | null = hasAlert ? 'alert' : hasBusy ? 'busy' : null;

  const expanded = Boolean(query) || isOpen;

  const handleToggle = (e: React.MouseEvent) => {
    e.stopPropagation();
    onToggle(group.key);
  };

  const handleSelect = () => actions.onSelectGroup(group);

  const handleContextMenu = (e: React.MouseEvent) => onGroupContextMenu(e, group);

  const handleNewTerminal = (e: React.MouseEvent) => {
    e.stopPropagation();
    actions.onNewTerminal(group);
  };

  const handleNewNote = (e: React.MouseEvent) => {
    e.stopPropagation();
    actions.onNewNote(group);
  };

  const handleFixFrame = (e: React.MouseEvent) => {
    e.stopPropagation();
    actions.onFixFrame(group);
  };

  const startFrameRename = (e: React.MouseEvent) => {
    e.stopPropagation();
    setFrameDraft(group.title);
    setRenamingFrame(true);
  };

  const commitFrameRename = () => {
    if (renamingFrame && group.frameId && frameDraft.trim()) {
      actions.onRenameFrame(group.frameId, frameDraft.trim());
    }
    setRenamingFrame(false);
  };

  const handleFrameDraftChange = (e: React.ChangeEvent<HTMLInputElement>) => setFrameDraft(e.target.value);

  const handleFrameRenameKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === 'Enter') commitFrameRename();
    if (e.key === 'Escape') setRenamingFrame(false);
  };

  return (
    <div className={styles.groupBlock}>
      <div
        onClick={handleSelect}
        className={styles.groupRow}
        onContextMenu={handleContextMenu}
        data-active={isActive || undefined}
      >
        <div className={styles.groupMain}>
          <button
            type="button"
            className={styles.caretBtn}
            onClick={handleToggle}
            aria-label={expanded ? 'Collapse group' : 'Expand group'}
          >
            {expanded ? <ChevronDown size={13} strokeWidth={2.25} /> : <ChevronRight size={13} strokeWidth={2.25} />}
          </button>
          {group.kind === 'frame' && renamingFrame ? (
            <input
              autoFocus
              value={frameDraft}
              onBlur={commitFrameRename}
              onClick={stopPropagation}
              onKeyDown={handleFrameRenameKey}
              onChange={handleFrameDraftChange}
              className={styles.groupRenameInput}
            />
          ) : (
            <span
              className={styles.groupTitle}
              title={group.path ?? group.title}
              onDoubleClick={group.kind === 'frame' ? startFrameRename : undefined}
            >
              {group.title}
            </span>
          )}
          {group.path && branch && (
            <span className={styles.branchBadge} title={branch}>
              <GitBranch size={10} strokeWidth={2} />
              <span>{branch}</span>
            </span>
          )}
        </div>

        <div className={styles.groupRight}>
          {groupSignal && <span className={styles.statusDot} data-kind={groupSignal} />}
          {(group.kind === 'path' || group.kind === 'loose') && (
            <button
              type="button"
              onClick={handleFixFrame}
              className={styles.warningBtn}
              aria-label="No frame on canvas"
              data-tooltip="No frame on canvas. Click to create one"
              data-tooltip-place="bottom"
            >
              <TriangleAlert size={13} strokeWidth={1.75} />
            </button>
          )}
          <div className={styles.countSlot}>
            <span className={styles.tileCountBadge}>{allTiles.length}</span>
            <div className={styles.groupHoverActions}>
              <button
                type="button"
                onClick={handleNewTerminal}
                className={styles.miniBtn}
                aria-label="New terminal in group"
                data-tooltip="New terminal"
                data-tooltip-place="bottom"
              >
                <Plus size={13} strokeWidth={2} />
              </button>
              <button
                type="button"
                onClick={handleNewNote}
                className={styles.miniBtn}
                aria-label="New note in group"
                data-tooltip="New note"
                data-tooltip-place="bottom"
              >
                <StickyNote size={12} strokeWidth={2} />
              </button>
            </div>
          </div>
        </div>
      </div>

      {expanded && (
        <div className={styles.tileList}>
          {visibleTiles.map((tile) => (
            <TileRow
              key={tile.id}
              tile={tile}
              isActive={activeTileId === tile.id}
              isDrifted={group.drifted.includes(tile.id)}
              isOutside={false}
              frameId={group.frameId}
              groupKey={group.key}
              signals={signals}
              actions={actions}
              onContextMenu={onTileContextMenu}
            />
          ))}
          {visibleOutside.map((tile) => (
            <TileRow
              key={tile.id}
              tile={tile}
              isActive={activeTileId === tile.id}
              isDrifted={false}
              isOutside={true}
              frameId={group.frameId}
              groupKey={group.key}
              signals={signals}
              actions={actions}
              onContextMenu={onTileContextMenu}
            />
          ))}
        </div>
      )}
    </div>
  );
};

interface GroupMenuState {
  x: number;
  y: number;
  group: ProjectGroup;
}

interface TileMenuState {
  x: number;
  y: number;
  tile: Tile;
}

const ProjectsPanel = ({ groups, activeTileId, activeGroupKey, recentProjects, signals, actions }: ProjectsPanelProps) => {
  const [query, setQuery] = React.useState('');
  const [sort, setSort] = React.useState<Sort>(savedSort);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(savedCollapsed);
  const [groupMenu, setGroupMenu] = React.useState<GroupMenuState | null>(null);
  const [tileMenu, setTileMenu] = React.useState<TileMenuState | null>(null);
  const [now, setNow] = React.useState(Date.now);

  const activity = React.useSyncExternalStore(subscribeTileActivity, getTileActivity);

  React.useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(id);
  }, []);

  const needle = query.trim().toLowerCase();

  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);

  const pickSort = (next: Sort) => {
    setSort(next);
    setPref(SORT_KEY, next);
  };

  const toggleCollapsed = (key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      setPref(COLLAPSED_KEY, JSON.stringify([...next]));
      return next;
    });
  };

  const allTiles = React.useMemo(() => {
    const seen = new Set<string>();
    const result: Tile[] = [];
    for (const group of groups) {
      for (const tile of group.tiles) {
        if (!seen.has(tile.id)) { seen.add(tile.id); result.push(tile); }
      }
      for (const tile of group.outside) {
        if (!seen.has(tile.id)) { seen.add(tile.id); result.push(tile); }
      }
    }
    return result;
  }, [groups]);

  const tileGroupMap = React.useMemo(() => {
    const map = new Map<string, ProjectGroup>();
    for (const group of groups) {
      for (const tile of group.tiles) map.set(tile.id, group);
      for (const tile of group.outside) map.set(tile.id, group);
    }
    return map;
  }, [groups]);

  const sortedFlatTiles = React.useMemo(() => {
    const filtered = needle
      ? allTiles.filter((t) => tileLabel(t).toLowerCase().includes(needle) || (tileGroupMap.get(t.id)?.title.toLowerCase().includes(needle) ?? false))
      : allTiles;

    if (sort === 'recent') {
      return [...filtered].sort((a, b) => {
        const at = activity.get(a.id)?.at ?? 0;
        const bt = activity.get(b.id)?.at ?? 0;
        return bt - at;
      });
    }

    return [...filtered].sort((a, b) => {
      const sa = tileStatus(signals.alerts.get(a.id), signals.agents.get(a.id));
      const sb = tileStatus(signals.alerts.get(b.id), signals.agents.get(b.id));
      const ra = sa ? STATUS_RANK[sa] : -1;
      const rb = sb ? STATUS_RANK[sb] : -1;
      if (ra !== rb) return rb - ra;
      const at = activity.get(a.id)?.at ?? 0;
      const bt = activity.get(b.id)?.at ?? 0;
      return bt - at;
    });
  }, [allTiles, sort, needle, activity, signals, tileGroupMap, now]);

  const attentionCount = React.useMemo(
    () => allTiles.filter((t) => tileStatus(signals.alerts.get(t.id), signals.agents.get(t.id))).length,
    [allTiles, signals]
  );

  const totalGroups = groups.length;

  const openGroupContextMenu = (e: React.MouseEvent, group: ProjectGroup) => {
    e.preventDefault();
    setGroupMenu({ x: e.clientX, y: e.clientY, group });
  };

  const openTileContextMenu = (e: React.MouseEvent, tile: Tile) => {
    e.preventDefault();
    setTileMenu({ x: e.clientX, y: e.clientY, tile });
  };

  const closeGroupMenu = () => setGroupMenu(null);
  const closeTileMenu = () => setTileMenu(null);

  const buildGroupMenuItems = (group: ProjectGroup): ContextMenuEntry[] => {
    const items: ContextMenuEntry[] = [
      {
        label: 'New terminal',
        icon: <SquareTerminal size={15} strokeWidth={1.75} />,
        onSelect: () => actions.onNewTerminal(group)
      },
      {
        label: 'New note',
        icon: <StickyNote size={15} strokeWidth={1.75} />,
        onSelect: () => actions.onNewNote(group)
      }
    ];

    if (group.kind === 'frame' && group.frameId) {
      items.push('separator');
      items.push({
        label: 'Rename frame',
        icon: <SquareDashed size={15} strokeWidth={1.75} />,
        onSelect: () => { if (group.frameId) actions.onRenameFrame(group.frameId, group.title); }
      });
      items.push({
        label: 'Set folder...',
        icon: <FolderPlus size={15} strokeWidth={1.75} />,
        onSelect: () => { if (group.frameId) actions.onSetFolder(group.frameId); }
      });
      if (group.path) {
        items.push('separator');
        items.push({
          label: 'Reveal in explorer',
          icon: <FolderOpen size={15} strokeWidth={1.75} />,
          onSelect: () => { if (group.path) revealPath(group.path); }
        });
        items.push({
          label: 'Copy path',
          icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
          onSelect: () => { if (group.path) void writeClipboard(group.path); }
        });
      }
      items.push('separator');
      items.push({
        label: 'Delete frame',
        icon: <Trash2 size={15} strokeWidth={1.75} />,
        danger: true,
        onSelect: () => { if (group.frameId) actions.onDeleteFrame(group.frameId); }
      });
    } else {
      if (group.path) {
        items.push('separator');
        items.push({
          label: 'Create frame',
          icon: <SquareDashed size={15} strokeWidth={1.75} />,
          onSelect: () => actions.onFixFrame(group)
        });
        items.push('separator');
        items.push({
          label: 'Reveal in explorer',
          icon: <FolderOpen size={15} strokeWidth={1.75} />,
          onSelect: () => { if (group.path) revealPath(group.path); }
        });
        items.push({
          label: 'Copy path',
          icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
          onSelect: () => { if (group.path) void writeClipboard(group.path); }
        });
        if (actions.onRemoveProject) {
          items.push('separator');
          items.push({
            label: 'Remove project',
            icon: <Trash2 size={15} strokeWidth={1.75} />,
            danger: true,
            onSelect: () => { if (group.path) actions.onRemoveProject!(group.path); }
          });
        }
      }
    }

    return items;
  };

  const buildTileMenuItems = (tile: Tile): ContextMenuEntry[] => [
    {
      label: 'Reveal in explorer',
      icon: <FolderOpen size={15} strokeWidth={1.75} />,
      disabled: !tile.cwd || isRemote(),
      onSelect: () => { if (tile.cwd) revealPath(tile.cwd); }
    },
    {
      label: 'Copy path',
      icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
      disabled: !tile.cwd,
      onSelect: () => { if (tile.cwd) void writeClipboard(tile.cwd); }
    },
    'separator',
    {
      label: 'Close tile',
      icon: <X size={15} strokeWidth={2} />,
      danger: true,
      onSelect: () => actions.onCloseTile(tile.id)
    }
  ];

  const handleNewTerminalHeader = () => actions.onNewTerminal(null);

  const isEmpty = groups.length === 0;

  return (
    <>
      <div className={styles.header}>
        <div className={styles.headerTitleWrap}>
          <span className={styles.headerTitle}>Projects</span>
          <span className={styles.countBadge}>{totalGroups}</span>
        </div>
        <div className={styles.headerActions}>
          <button
            type="button"
            className={styles.iconBtn}
            onClick={actions.onNewFrame}
            aria-label="New frame"
            data-tooltip="New frame"
            data-tooltip-place="bottom"
          >
            <SquareDashed size={14} strokeWidth={1.75} />
          </button>
          <button
            type="button"
            className={styles.iconBtn}
            onClick={actions.onAddProject}
            aria-label="Add project folder"
            data-tooltip="Add project folder"
            data-tooltip-place="bottom"
          >
            <FolderPlus size={14} strokeWidth={1.75} />
          </button>
          <button
            type="button"
            className={styles.iconBtn}
            onClick={handleNewTerminalHeader}
            aria-label="New terminal"
            data-tooltip="New terminal"
            data-tooltip-place="bottom"
          >
            <Plus size={14} strokeWidth={2} />
          </button>
        </div>
      </div>

      <div className={styles.toolbarWrap}>
        <div className={styles.filter}>
          <Search size={12} strokeWidth={2} />
          <input value={query} onChange={handleQueryChange} placeholder="Filter groups & tiles" />
        </div>
        <div className={styles.sortBar}>
          <button
            type="button"
            className={styles.sortBtn}
            onClick={() => pickSort('projects')}
            data-active={sort === 'projects' || undefined}
          >
            Projects
          </button>
          <button
            type="button"
            className={styles.sortBtn}
            onClick={() => pickSort('recent')}
            data-active={sort === 'recent' || undefined}
          >
            Recent
          </button>
          <button
            type="button"
            className={styles.sortBtn}
            onClick={() => pickSort('attention')}
            data-active={sort === 'attention' || undefined}
          >
            Attention
            {attentionCount > 0 && <span className={styles.sortCount}>{attentionCount}</span>}
          </button>
        </div>
      </div>

      <div className={styles.body}>
        {isEmpty ? (
          <div className={styles.emptyState}>
            <span className={styles.emptyText}>No projects or tiles yet</span>
            <button type="button" className={styles.addProjectBtn} onClick={actions.onAddProject}>
              <FolderPlus size={14} strokeWidth={2} />
              Add project
            </button>
            {recentProjects.length > 0 && (
              <div className={styles.recentSection}>
                <span className={styles.recentHeading}>
                  <Clock size={11} strokeWidth={2} />
                  Recent
                </span>
                {recentProjects.map((path) => {
                  const pickRecent = () => actions.onSelectGroup({ key: `path:${path}`, kind: 'path', title: projectName(path), path, frameId: null, tiles: [], outside: [], drifted: [] });
                  return (
                    <button key={path} type="button" className={styles.recentRow} onClick={pickRecent}>
                      <Folder size={14} strokeWidth={1.75} />
                      <span className={styles.recentInfo}>
                        <span className={styles.recentName}>{projectName(path)}</span>
                        <span className={styles.recentPath}>{path}</span>
                      </span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        ) : sort === 'projects' ? (
          groups.map((group) => (
            <GroupRow
              key={group.key}
              group={group}
              query={needle}
              isOpen={!collapsed.has(group.key)}
              isActive={group.key === activeGroupKey}
              activeTileId={activeTileId}
              signals={signals}
              actions={actions}
              onToggle={toggleCollapsed}
              onGroupContextMenu={openGroupContextMenu}
              onTileContextMenu={openTileContextMenu}
            />
          ))
        ) : sortedFlatTiles.length === 0 ? (
          <div className={styles.empty}>No tiles</div>
        ) : (
          sortedFlatTiles.map((tile) => {
            const group = tileGroupMap.get(tile.id);
            return (
              <TileRow
                key={tile.id}
                tile={tile}
                isActive={activeTileId === tile.id}
                isDrifted={group ? group.drifted.includes(tile.id) : false}
                isOutside={group ? group.outside.some((t) => t.id === tile.id) : false}
                frameId={group?.frameId ?? null}
                groupKey={group?.key ?? ''}
                signals={signals}
                actions={actions}
                flatGroupTitle={group?.title}
                onContextMenu={openTileContextMenu}
              />
            );
          })
        )}
      </div>

      {groupMenu && (
        <ContextMenu
          x={groupMenu.x}
          y={groupMenu.y}
          onClose={closeGroupMenu}
          items={buildGroupMenuItems(groupMenu.group)}
        />
      )}
      {tileMenu && (
        <ContextMenu
          x={tileMenu.x}
          y={tileMenu.y}
          onClose={closeTileMenu}
          items={buildTileMenuItems(tileMenu.tile)}
        />
      )}
    </>
  );
};

export default ProjectsPanel;
export type { ProjectsPanelProps } from './types';
