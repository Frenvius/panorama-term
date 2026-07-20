import React from 'react';
import type { LucideIcon } from 'lucide-react';
import {
  Plus,
  Code,
  Globe,
  Image,
  Layers,
  Search,
  Trash2,
  Pencil,
  Network,
  Container,
  GitBranch,
  Crosshair,
  FolderTree,
  StickyNote,
  FolderOpen,
  ChevronRight,
  ClipboardCopy,
  PanelLeftClose,
  SquareTerminal
} from 'lucide-react';

import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';
import type { TileType } from '~/domain/interfaces/workspace.interface';
import type { DirEntry } from '~/adapter/fs/fs.client';
import type { NotifyKind } from '~/components/commons/Notifications/bridge';
import GitTab from '~/components/Canvas/Navigator/GitTab';
import FileTree from '~/components/Canvas/Navigator/FileTree';
import DockerTab from '~/components/Canvas/Navigator/DockerTab';
import ClaudeLogo from '~/components/commons/ClaudeLogo';
import ContextMenu from '~/components/commons/ContextMenu';
import { tileLabel } from '~/usecase/util/title';
import { groupByFrame } from '~/usecase/util/frame';
import { revealPath } from '~/adapter/shell/shell.client';
import { dockerAvailable } from '~/adapter/docker/docker.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { getBinding, formatCombo } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

const TYPE_ICON: Record<TileType, { Icon: LucideIcon; color: string }> = {
  term: { Icon: SquareTerminal, color: '#7aab6e' },
  note: { Icon: StickyNote, color: '#8a7aab' },
  code: { Icon: Code, color: '#7a8aab' },
  image: { Icon: Image, color: '#c07a6e' },
  graph: { Icon: Network, color: '#c8a35a' },
  browser: { Icon: Globe, color: '#5c9bcf' }
};

interface Menu {
  x: number;
  y: number;
  tile?: Tile;
  entry?: DirEntry;
}

interface NavigatorProps {
  tiles: Tile[];
  frames: Frame[];
  activeTile: string | null;
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
  onNewTile: () => void;
  onFocusTile: (id: string, zoomToMax?: boolean) => void;
  onFocusFrame: (id: string) => void;
  onRenameTile: (id: string, title: string) => void;
  onCloseTile: (id: string) => void;
  activeDiff: string | null;
  onDiffFiles: (files: string[]) => void;
  onOpenDiff: (root: string, file: string) => void;
  onClose: () => void;
}

const WIDTH_KEY = 'panorama:navWidth';
const COLLAPSED_KEY = 'panorama:navCollapsed';
const TAB_KEY = 'panorama:navTab';
const TABS = ['files', 'tiles', 'git', 'docker'] as const;
type Tab = (typeof TABS)[number];

const savedTab = (): Tab => {
  const raw = localStorage.getItem(TAB_KEY);
  return TABS.includes(raw as Tab) ? (raw as Tab) : 'files';
};
const MIN_WIDTH = 200;
const MAX_WIDTH = 800;

const savedWidth = (): number => {
  const raw = Number(localStorage.getItem(WIDTH_KEY));
  return raw >= MIN_WIDTH && raw <= MAX_WIDTH ? raw : 248;
};

const savedCollapsed = (): Set<string> => {
  try {
    return new Set(JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? '[]'));
  } catch {
    return new Set();
  }
};

const Navigator = ({
  tiles,
  frames,
  activeTile,
  alerts,
  agents,
  onNewTile,
  onFocusTile,
  onFocusFrame,
  onRenameTile,
  onCloseTile,
  activeDiff,
  onDiffFiles,
  onOpenDiff,
  onClose
}: NavigatorProps) => {
  const [tab, setTab] = React.useState<Tab>(savedTab);
  const [hasDocker, setHasDocker] = React.useState(false);

  React.useEffect(() => {
    localStorage.setItem(TAB_KEY, tab);
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
  const [collapsed, setCollapsed] = React.useState<Set<string>>(savedCollapsed);
  const [renaming, setRenaming] = React.useState<string | null>(null);
  const [draft, setDraft] = React.useState('');
  const [menu, setMenu] = React.useState<Menu | null>(null);

  React.useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--nav-inset', `${width + 8}px`);
    return () => root.style.setProperty('--nav-inset', '0px');
  }, [width]);

  const needle = query.trim().toLowerCase();
  const { members, loose } = React.useMemo(() => groupByFrame(frames, tiles), [frames, tiles]);

  const cwd = tiles.find((t) => t.id === activeTile)?.cwd ?? null;
  const lastCwd = React.useRef<string | null>(null);
  if (cwd) lastCwd.current = cwd;
  const root = cwd ?? lastCwd.current;

  const matches = (tile: Tile): boolean => !needle || tileLabel(tile).toLowerCase().includes(needle);

  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);

  const newTile = () => onNewTile();

  const showTiles = () => setTab('tiles');
  const showFiles = () => setTab('files');
  const showGit = () => setTab('git');
  const showDocker = () => setTab('docker');

  const toggleFrame = (id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...next]));
      return next;
    });
  };

  const startRename = (tile: Tile) => {
    setDraft(tile.userTitle || tileLabel(tile));
    setRenaming(tile.id);
  };

  const commitRename = () => {
    if (renaming) onRenameTile(renaming, draft.trim());
    setRenaming(null);
  };

  const onRenameKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') commitRename();
    if (e.key === 'Escape') setRenaming(null);
  };

  const openTileMenu = (e: React.MouseEvent, tile: Tile) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, tile });
  };

  const openFileMenu = (e: React.MouseEvent, entry: DirEntry) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, entry });
  };

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
      localStorage.setItem(WIDTH_KEY, String(next));
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };

  const menuItems = () => {
    if (menu?.tile) {
      const tile = menu.tile;
      return [
        { label: 'Rename', icon: <Pencil size={15} strokeWidth={1.75} />, onSelect: () => startRename(tile) },
        { label: 'Focus', icon: <Crosshair size={15} strokeWidth={1.75} />, onSelect: () => onFocusTile(tile.id, true) },
        {
          label: 'Reveal in explorer',
          icon: <FolderOpen size={15} strokeWidth={1.75} />,
          onSelect: () => tile.cwd && revealPath(tile.cwd),
          disabled: !tile.cwd
        },
        { label: 'Close', icon: <Trash2 size={15} strokeWidth={1.75} />, danger: true, onSelect: () => onCloseTile(tile.id) }
      ];
    }
    if (menu?.entry) {
      const entry = menu.entry;
      return [
        { label: 'Open', icon: <FolderOpen size={15} strokeWidth={1.75} />, onSelect: () => revealPath(entry.path) },
        {
          label: 'Copy path',
          icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
          onSelect: () => writeClipboard(entry.path)
        }
      ];
    }
    return [];
  };

  const row = (tile: Tile, nested: boolean) => {
    const { Icon, color } = TYPE_ICON[tile.type] ?? TYPE_ICON.term;
    const agent = agents.get(tile.id);
    const select = () => onFocusTile(tile.id);
    const zoom = () => onFocusTile(tile.id, true);
    const contextMenu = (e: React.MouseEvent) => openTileMenu(e, tile);

    return (
      <div
        key={tile.id}
        className={styles.tile}
        onClick={select}
        onDoubleClick={zoom}
        onContextMenu={contextMenu}
        data-nested={nested || undefined}
        data-active={tile.id === activeTile || undefined}
      >
        <Icon size={13} strokeWidth={1.75} style={{ color }} className={styles.icon} />
        {renaming === tile.id ? (
          <input
            autoFocus
            className={styles.rename}
            value={draft}
            onBlur={commitRename}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={onRenameKey}
            onClick={(e) => e.stopPropagation()}
          />
        ) : (
          <span className={styles.name}>{tileLabel(tile)}</span>
        )}
        {alerts.has(tile.id) && <span className={styles.dot} />}
        {agent && (
          <span className={styles.agent} data-tooltip={agent === 'busy' ? 'Agent working' : 'Agent idle'}>
            {agent === 'busy' ? (
              <span className={styles.dots}>
                <span />
                <span />
                <span />
              </span>
            ) : (
              <ClaudeLogo size={11} />
            )}
          </span>
        )}
      </div>
    );
  };

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
            onOpenDiff={(file) => onOpenDiff(root, file)}
          />
        ) : (
          <div className={styles.empty}>Focus a terminal to see its repo</div>
        ))}

      {tab === 'docker' && <DockerTab query={needle} />}

      <div className={styles.body} style={{ display: tab === 'git' || tab === 'docker' ? 'none' : undefined }}>
        {tab === 'tiles' && (
          <>
            {frames.map((frame) => {
              const children = (members.get(frame.id) ?? []).filter(matches);
              const shut = collapsed.has(frame.id);
              const alerted = children.some((c) => alerts.has(c.id));
              const frameAgent = children.some((c) => agents.get(c.id) === 'busy')
                ? 'busy'
                : children.some((c) => agents.get(c.id))
                  ? 'idle'
                  : null;
              const toggle = () => toggleFrame(frame.id);
              const goto = (e: React.MouseEvent) => {
                e.stopPropagation();
                onFocusFrame(frame.id);
              };

              return (
                <div key={frame.id}>
                  <div className={styles.group} onClick={toggle}>
                    <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} data-open={!shut || undefined} />
                    <span className={styles.swatch} style={{ background: frame.color }} />
                    <span className={styles.name}>{frame.title}</span>
                    {alerted && <span className={styles.dot} />}
                    {frameAgent && (
                      <span className={styles.agent} data-tooltip={frameAgent === 'busy' ? 'Agent working' : 'Agent idle'}>
                        {frameAgent === 'busy' ? (
                          <span className={styles.dots}>
                            <span />
                            <span />
                            <span />
                          </span>
                        ) : (
                          <ClaudeLogo size={11} />
                        )}
                      </span>
                    )}
                    <span className={styles.count}>{children.length}</span>
                    <button className={styles.goto} onClick={goto} aria-label="Go to frame">
                      <Crosshair size={12} strokeWidth={2} />
                    </button>
                  </div>
                  {!shut && children.map((tile) => row(tile, true))}
                </div>
              );
            })}
            {loose.filter(matches).map((tile) => row(tile, false))}
            {tiles.length === 0 && <div className={styles.empty}>No tiles on canvas</div>}
          </>
        )}

        {tab === 'files' &&
          (root ? (
            <FileTree key={root} root={root} query={needle} onOpen={revealPath} onMenu={openFileMenu} />
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

      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu} items={menuItems()} />}
    </div>
  );
};

export default Navigator;
