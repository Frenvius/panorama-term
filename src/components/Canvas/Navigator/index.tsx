import React from 'react';
import {
  Plus,
  Layers,
  Search,
  Pencil,
  FilePlus,
  Container,
  GitBranch,
  FolderTree,
  FolderOpen,
  FolderPlus,
  ClipboardCopy,
  PanelLeftClose
} from 'lucide-react';

import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';
import type { DirEntry } from '~/adapter/fs/fs.client';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { Creating } from '~/components/Canvas/Navigator/FileTree';
import { getPref, setPref } from '~/usecase/util/prefs';
import GitTab from '~/components/Canvas/Navigator/GitTab';
import TilesTab from '~/components/Canvas/Navigator/TilesTab';
import FileTree from '~/components/Canvas/Navigator/FileTree';
import DockerTab from '~/components/Canvas/Navigator/DockerTab';
import ContextMenu from '~/components/commons/ContextMenu';
import { revealPath } from '~/adapter/shell/shell.client';
import { dockerAvailable } from '~/adapter/docker/docker.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { getBinding, formatCombo } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

interface Menu {
  x: number;
  y: number;
  entry: DirEntry;
}

export interface NavigatorHandlers {
  onNewTile: () => void;
  onFocusTile: (id: string, zoomToMax?: boolean) => void;
  onFocusFrame: (id: string) => void;
  onRenameTile: (id: string, title: string) => void;
  onCloseTile: (id: string) => void;
  onDiffFiles: (files: string[]) => void;
  onOpenDiff: (root: string, file: string, commit?: string) => void;
  onOpenFile: (root: string, file: string, preview?: boolean) => void;
  onClose: () => void;
}

interface NavigatorProps {
  tiles: Tile[];
  frames: Frame[];
  activeTile: string | null;
  activeDiff: string | null;
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
  handlers: NavigatorHandlers;
}

const WIDTH_KEY = 'panorama:navWidth';
const TAB_KEY = 'panorama:navTab';
const TABS = ['files', 'tiles', 'git', 'docker'] as const;
type Tab = (typeof TABS)[number];

const savedTab = (): Tab => {
  const raw = getPref(TAB_KEY);
  return TABS.includes(raw as Tab) ? (raw as Tab) : 'files';
};
const MIN_WIDTH = 200;
const MAX_WIDTH = 800;

const savedWidth = (): number => {
  const raw = Number(getPref(WIDTH_KEY));
  return raw >= MIN_WIDTH && raw <= MAX_WIDTH ? raw : 248;
};

const Navigator = ({ tiles, frames, activeTile, activeDiff, alerts, agents, handlers }: NavigatorProps) => {
  const { onNewTile, onFocusTile, onFocusFrame, onRenameTile, onCloseTile, onDiffFiles, onOpenDiff, onOpenFile, onClose } =
    handlers;
  const [tab, setTab] = React.useState<Tab>(savedTab);
  const [hasDocker, setHasDocker] = React.useState(false);

  React.useEffect(() => {
    setPref(TAB_KEY, tab);
  }, [tab]);
  const [width, setWidth] = React.useState(savedWidth);

  React.useEffect(() => {
    dockerAvailable()
      .then((ok) => {
        setHasDocker(ok);
        if (!ok) setTab((t) => (t === 'docker' ? 'files' : t));
      })
      .catch(() => setHasDocker(false));
  }, []);
  const [query, setQuery] = React.useState('');
  const [menu, setMenu] = React.useState<Menu | null>(null);
  const [creating, setCreating] = React.useState<Creating | null>(null);

  React.useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--nav-inset', `${width + 8}px`);
    return () => root.style.setProperty('--nav-inset', '0px');
  }, [width]);

  const needle = query.trim().toLowerCase();

  const cwd = tiles.find((t) => t.id === activeTile)?.cwd ?? null;
  const lastCwd = React.useRef<string | null>(null);
  if (cwd) lastCwd.current = cwd;
  const root = cwd ?? lastCwd.current;

  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);

  const newTile = () => onNewTile();

  const showTiles = () => setTab('tiles');
  const showFiles = () => setTab('files');
  const showGit = () => setTab('git');
  const showDocker = () => setTab('docker');

  const tileSignals = React.useMemo(() => ({ alerts, agents }), [alerts, agents]);
  const tilesHandlers = { onCloseTile, onFocusTile, onFocusFrame, onRenameTile };

  const openFile = (path: string, preview = false) => {
    if (root) onOpenFile(root, path, preview);
  };

  const openFileMenu = (e: React.MouseEvent, entry: DirEntry) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const openBlankMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (tab !== 'files' || !root || e.target !== e.currentTarget) return;
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
      next = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, startW + ev.clientX - startX));
      setWidth(next);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      setPref(WIDTH_KEY, String(next));
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };

  const menuItems = (entry: DirEntry) => [
    { label: 'New file', icon: <FilePlus size={15} strokeWidth={1.75} />, onSelect: () => startCreate(entry, false) },
    { label: 'New folder', icon: <FolderPlus size={15} strokeWidth={1.75} />, onSelect: () => startCreate(entry, true) },
    {
      label: 'Edit',
      icon: <Pencil size={15} strokeWidth={1.75} />,
      onSelect: () => openFile(entry.path),
      disabled: entry.dir
    },
    { label: 'Reveal in explorer', icon: <FolderOpen size={15} strokeWidth={1.75} />, onSelect: () => revealPath(entry.path) },
    {
      label: 'Copy path',
      icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
      onSelect: () => writeClipboard(entry.path)
    }
  ];

  return (
    <div className={styles.panel} style={{ width }}>
      <div className={styles.header}>
        <div className={styles.tabs}>
          <button
            className={styles.tab}
            onClick={showFiles}
            aria-label="Files"
            data-tooltip="Files"
            data-tooltip-place="bottom"
            data-active={tab === 'files' || undefined}
          >
            <FolderTree size={14} strokeWidth={2} />
          </button>
          <button
            className={styles.tab}
            onClick={showTiles}
            aria-label="Tiles"
            data-tooltip="Tiles"
            data-tooltip-place="bottom"
            data-active={tab === 'tiles' || undefined}
          >
            <Layers size={14} strokeWidth={2} />
          </button>
          <button
            className={styles.tab}
            onClick={showGit}
            aria-label="Git"
            data-tooltip="Git"
            data-tooltip-place="bottom"
            data-active={tab === 'git' || undefined}
          >
            <GitBranch size={14} strokeWidth={2} />
          </button>
          {hasDocker && (
            <button
              className={styles.tab}
              onClick={showDocker}
              aria-label="Docker"
              data-tooltip="Docker"
              data-tooltip-place="bottom"
              data-active={tab === 'docker' || undefined}
            >
              <Container size={14} strokeWidth={2} />
            </button>
          )}
        </div>
        <button
          className={styles.action}
          onClick={onClose}
          aria-label="Close menu"
          data-tooltip="Close menu"
          data-tooltip-place="bottom"
        >
          <PanelLeftClose size={14} strokeWidth={1.75} />
        </button>
      </div>

      {tab !== 'git' && (
        <div className={styles.search}>
          <div className={styles.filter}>
            <Search size={12} strokeWidth={2} />
            <input value={query} onChange={onQuery} placeholder="Filter" />
          </div>
          {tab === 'tiles' && (
            <button
              className={styles.new}
              onClick={newTile}
              aria-label="New terminal"
              data-tooltip="New terminal"
              data-tooltip-place="bottom"
            >
              <Plus size={14} strokeWidth={2} />
            </button>
          )}
        </div>
      )}

      {tab === 'git' &&
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

      {tab === 'docker' && <DockerTab query={needle} />}

      {tab === 'tiles' && (
        <TilesTab
          tiles={tiles}
          frames={frames}
          query={needle}
          signals={tileSignals}
          activeTile={activeTile}
          handlers={tilesHandlers}
        />
      )}

      <div
        className={styles.body}
        onContextMenu={openBlankMenu}
        style={{ display: tab === 'files' ? undefined : 'none' }}
      >
        {tab === 'files' &&
          (root ? (
            <FileTree key={root} root={root} query={needle} creating={creating} handlers={fileTreeHandlers} />
          ) : (
            <div className={styles.empty}>Focus a terminal to see its folder</div>
          ))}
      </div>

      <div className={styles.resizer} onPointerDown={startResize} />
      <div className={styles.toggleZone}>
        <button
          className={styles.toggle}
          onClick={onClose}
          data-tooltip="Hide menu"
          data-shortcut={formatCombo(getBinding('view.navigator'))}
          aria-label="Hide menu"
        />
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu} items={menuItems(menu.entry)} />}
    </div>
  );
};

export default Navigator;
