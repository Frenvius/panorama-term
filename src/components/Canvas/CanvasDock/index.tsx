import { isRemote } from '~/adapter/transport';
import type { DirEntry } from '~/adapter/fs/fs.client';
import type { DockSide } from '~/domain/interfaces/dock.interface';
import type { Tile } from '~/domain/interfaces/canvas.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { Creating } from '~/components/Canvas/Navigator/FileTree';
import type { PanelDescriptor } from '~/components/commons/Dock/panels';
import type { CanvasSidebarMode } from '~/usecase/util/canvasSidebars';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';
import type { ProjectsPanelActions, TidyState } from '~/components/commons/ProjectsPanel/types';

import React from 'react';
import {
  Search,
  Pencil,
  FilePlus,
  FolderOpen,
  FolderPlus,
  ClipboardCopy,
  PanelLeftClose,
  PanelRightClose
} from 'lucide-react';

import GitTab from '~/components/Canvas/Navigator/GitTab';
import ProjectsPanel from '~/components/commons/ProjectsPanel';
import FileTree from '~/components/Canvas/Navigator/FileTree';
import DockerTab from '~/components/Canvas/Navigator/DockerTab';
import DockStrip from '~/components/commons/Dock/DockStrip';
import ContextMenu from '~/components/commons/ContextMenu';
import { getPref, setPref } from '~/usecase/util/prefs';
import { useDockLayout } from '~/usecase/hooks/useDockLayout';
import { revealPath } from '~/adapter/shell/shell.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { getBinding, formatCombo } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

interface Menu {
  x: number;
  y: number;
  entry: DirEntry;
}

export interface CanvasDockHandlers {
  onClose: () => void;
  onWidth: (width: number) => void;
  onDiffFiles: (files: string[]) => void;
  onOpenDiff: (root: string, file: string, commit?: string) => void;
  onOpenFile: (root: string, file: string, preview?: boolean) => void;
}

interface CanvasDockProps {
  mode: CanvasSidebarMode;
  side: DockSide;
  tiles: Tile[];
  groups: ProjectGroup[];
  panels: PanelDescriptor[];
  activeTile: string | null;
  activeDiff: string | null;
  activeGroupKey: string | null;
  recentProjects: string[];
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
  handlers: CanvasDockHandlers;
  projectActions: ProjectsPanelActions;
  tidy?: TidyState;
}

const LEFT_WIDTH_KEY = 'panorama:navWidth';
const RIGHT_WIDTH_KEY = 'panorama:dockRightWidth';
const MIN_WIDTH = 200;
const MAX_WIDTH = 800;

const savedWidth = (side: DockSide): number => {
  const key = side === 'left' ? LEFT_WIDTH_KEY : RIGHT_WIDTH_KEY;
  const raw = Number(getPref(key));
  return raw >= MIN_WIDTH && raw <= MAX_WIDTH ? raw : 248;
};

const CanvasDock = ({ mode, side, tiles, groups, panels, activeTile, activeDiff, activeGroupKey, recentProjects, alerts, agents, handlers, projectActions, tidy }: CanvasDockProps) => {
  const {
    onClose,
    onWidth,
    onDiffFiles,
    onOpenDiff,
    onOpenFile
  } = handlers;

  const { layout } = useDockLayout('canvas');
  const rawPanel = layout.active[side];
  const activePanel = panels.some((p) => p.id === rawPanel) ? rawPanel : (panels[0]?.id ?? null);

  const widthKey = side === 'left' ? LEFT_WIDTH_KEY : RIGHT_WIDTH_KEY;
  const cssVar = side === 'left' ? '--nav-inset' : '--dock-right-inset';
  const closeCmd = side === 'left' ? 'view.navigator' : 'view.inspector';

  const [width, setWidth] = React.useState(() => savedWidth(side));
  const [query, setQuery] = React.useState('');
  const [menu, setMenu] = React.useState<Menu | null>(null);
  const [creating, setCreating] = React.useState<Creating | null>(null);

  React.useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty(cssVar, `${width + 8}px`);
    onWidth(width);
    return () => root.style.setProperty(cssVar, '0px');
  }, [width]);

  const needle = query.trim().toLowerCase();

  const cwd = tiles.find((t) => t.id === activeTile)?.cwd ?? null;
  const lastCwd = React.useRef<string | null>(null);
  if (cwd) lastCwd.current = cwd;
  const root = cwd ?? lastCwd.current;

  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);

  const tileSignals = React.useMemo(() => ({ alerts, agents }), [alerts, agents]);

  const openFile = (path: string, preview = false) => {
    if (root) onOpenFile(root, path, preview);
  };

  const openFileMenu = (e: React.MouseEvent, entry: DirEntry) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const openBlankMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (activePanel !== 'files' || !root || e.target !== e.currentTarget) return;
    setMenu({ x: e.clientX, y: e.clientY, entry: { name: '', path: root, dir: true } });
  };

  const startCreate = (entry: DirEntry, dir: boolean) => {
    const parent = entry.dir ? entry.path : entry.path.replace(/[\\/][^\\/]+$/, '');
    setCreating({ parent, dir });
  };

  const endCreate = (path?: string) => {
    if (path && !creating?.dir) openFile(path);
    setCreating(null);
  };

  const fileTreeHandlers = { onOpen: openFile, onMenu: openFileMenu, onCreateEnd: endCreate };

  const closeMenu = () => setMenu(null);

  const startResize = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    const startX = e.clientX;
    const startW = width;
    let next = startW;
    el.setPointerCapture(e.pointerId);

    const move = (ev: PointerEvent) => {
      const delta = side === 'left' ? ev.clientX - startX : startX - ev.clientX;
      next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startW + delta));
      setWidth(next);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      setPref(widthKey, String(next));
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };

  const menuItems = (entry: DirEntry) => [
    { label: 'New file', icon: <FilePlus size={15} strokeWidth={1.75} />, onSelect: () => startCreate(entry, false) },
    { label: 'New folder', icon: <FolderPlus size={15} strokeWidth={1.75} />, onSelect: () => startCreate(entry, true) },
    { label: 'Edit', icon: <Pencil size={15} strokeWidth={1.75} />, onSelect: () => openFile(entry.path), disabled: entry.dir },
    { label: 'Reveal in explorer', icon: <FolderOpen size={15} strokeWidth={1.75} />, onSelect: () => revealPath(entry.path), disabled: isRemote() },
    { label: 'Copy path', icon: <ClipboardCopy size={15} strokeWidth={1.75} />, onSelect: () => writeClipboard(entry.path) }
  ];

  const panelClass = side === 'left' ? styles.panelLeft : styles.panelRight;
  const resizerClass = side === 'left' ? styles.resizerRight : styles.resizerLeft;
  const toggleZoneClass = side === 'left' ? styles.toggleZoneRight : styles.toggleZoneLeft;
  const CloseIcon = side === 'left' ? PanelLeftClose : PanelRightClose;

  return (
    <div className={panelClass} style={{ width }} data-mode={mode}>
      <div className={styles.header}>
        <DockStrip mode="canvas" side={side} panels={panels} />
        <button
          className={styles.action}
          onClick={onClose}
          aria-label="Close panel"
          data-tooltip="Close panel"
          data-tooltip-place="bottom"
        >
          <CloseIcon size={14} strokeWidth={1.75} />
        </button>
      </div>

      {activePanel !== 'git' && activePanel !== 'projects' && (
        <div className={styles.search}>
          <div className={styles.filter}>
            <Search size={12} strokeWidth={2} />
            <input value={query} onChange={onQuery} placeholder="Filter" />
          </div>
        </div>
      )}

      {activePanel === 'git' &&
        (root ? (
          <GitTab
            key={root}
            root={root}
            query=""
            active={activeDiff}
            onFiles={onDiffFiles}
            onOpenDiff={onOpenDiff}
            onOpenFile={(file) => onOpenFile(root, file)}
          />
        ) : (
          <div className={styles.empty}>Focus a terminal to see its repo</div>
        ))}

      {activePanel === 'docker' && <DockerTab query={needle} />}

      {activePanel === 'projects' && (
        <ProjectsPanel
          groups={groups}
          activeTileId={activeTile}
          activeGroupKey={activeGroupKey}
          recentProjects={recentProjects}
          signals={tileSignals}
          actions={projectActions}
          tidy={tidy}
        />
      )}

      <div
        className={styles.body}
        onContextMenu={openBlankMenu}
        style={{ display: activePanel === 'files' ? undefined : 'none' }}
      >
        {root ? (
          <FileTree key={root} root={root} query={needle} creating={creating} handlers={fileTreeHandlers} />
        ) : (
          <div className={styles.empty}>Focus a terminal to see its folder</div>
        )}
      </div>

      <div className={resizerClass} onPointerDown={startResize} />
      <div className={toggleZoneClass}>
        <button
          className={styles.toggleHandle}
          onClick={onClose}
          data-tooltip="Hide panel"
          data-shortcut={formatCombo(getBinding(closeCmd))}
          aria-label="Hide panel"
        />
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu} items={menuItems(menu.entry)} />}
    </div>
  );
};

export default CanvasDock;
