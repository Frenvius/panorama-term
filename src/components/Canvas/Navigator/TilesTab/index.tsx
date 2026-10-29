import React from 'react';
import { Pencil, Trash2, Crosshair, FolderOpen, AlignJustify } from 'lucide-react';

import type { TileStatus } from '~/usecase/util/tileStatus';
import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { TileActivity } from '~/usecase/util/tileActivity';
import type { CardInfo, RenameField, TileCardHandlers } from '~/components/Canvas/Navigator/TilesTab/TileCard';
import { tileLabel } from '~/usecase/util/title';
import { tileStatus } from '~/usecase/util/tileStatus';
import { groupByFrame } from '~/usecase/util/frame';
import { getPref, setPref } from '~/usecase/util/prefs';
import { revealPath } from '~/adapter/shell/shell.client';
import ContextMenu from '~/components/commons/ContextMenu';
import TileCard from '~/components/Canvas/Navigator/TilesTab/TileCard';
import FrameGroup from '~/components/Canvas/Navigator/TilesTab/FrameGroup';
import { getTileActivity, subscribeTileActivity } from '~/usecase/util/tileActivity';

import styles from './styles.module.scss';

export interface TileSignals {
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
}

export interface TilesTabHandlers {
  onCloseTile: (id: string) => void;
  onFocusFrame: (id: string) => void;
  onRenameTile: (id: string, title: string) => void;
  onFocusTile: (id: string, zoomToMax?: boolean) => void;
}

interface TilesTabProps {
  tiles: Tile[];
  query: string;
  frames: Frame[];
  signals: TileSignals;
  activeTile: string | null;
  handlers: TilesTabHandlers;
}

interface Menu {
  x: number;
  y: number;
  tile: Tile;
}

const SCOPES = ['frames', 'recent', 'attention'] as const;
type Scope = (typeof SCOPES)[number];

const SCOPE_KEY = 'panorama:tilesScope';
const COMPACT_KEY = 'panorama:tilesCompact';
const COLLAPSED_KEY = 'panorama:navCollapsed';
const TICK_MS = 30000;
const STATUS_RANK: Record<TileStatus, number> = { waiting: 0, working: 1, done: 2 };

const savedScope = (): Scope => {
  const raw = getPref(SCOPE_KEY);
  return SCOPES.includes(raw as Scope) ? (raw as Scope) : 'frames';
};

const savedCollapsed = (): Set<string> => {
  try {
    return new Set(JSON.parse(getPref(COLLAPSED_KEY) ?? '[]'));
  } catch {
    return new Set();
  }
};

const haystack = (tile: Tile, activity?: TileActivity): string =>
  [tileLabel(tile), activity?.snippet?.text, activity?.snippet?.cmd, tile.cwd, tile.branch, tile.content, tile.url]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();

const TilesTab = ({ tiles, query, frames, signals, activeTile, handlers }: TilesTabProps) => {
  const { onCloseTile, onFocusFrame, onRenameTile, onFocusTile } = handlers;
  const activity = React.useSyncExternalStore(subscribeTileActivity, getTileActivity);
  const [scope, setScope] = React.useState<Scope>(savedScope);
  const [compact, setCompact] = React.useState(() => getPref(COMPACT_KEY) === '1');
  const [collapsed, setCollapsed] = React.useState<Set<string>>(savedCollapsed);
  const [renaming, setRenaming] = React.useState<string | null>(null);
  const [draft, setDraft] = React.useState('');
  const [menu, setMenu] = React.useState<Menu | null>(null);
  const [now, setNow] = React.useState(Date.now);

  React.useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), TICK_MS);
    return () => clearInterval(id);
  }, []);

  const { members, loose } = React.useMemo(() => groupByFrame(frames, tiles), [frames, tiles]);

  const frameOf = React.useMemo(() => {
    const map = new Map<string, Frame>();
    frames.forEach((f) => (members.get(f.id) ?? []).forEach((t) => map.set(t.id, f)));
    return map;
  }, [frames, members]);

  const statusOf = (tile: Tile) => tileStatus(signals.alerts.get(tile.id), signals.agents.get(tile.id));
  const matches = (tile: Tile) => !query || query.split(/\s+/).every((w) => haystack(tile, activity.get(tile.id)).includes(w));
  const attentionCount = tiles.filter((t) => statusOf(t)).length;

  const pickScope = (next: Scope) => {
    setScope(next);
    setPref(SCOPE_KEY, next);
  };

  const toggleCompact = () => {
    setCompact((v) => {
      setPref(COMPACT_KEY, v ? '0' : '1');
      return !v;
    });
  };

  const toggleFrame = (id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      setPref(COLLAPSED_KEY, JSON.stringify([...next]));
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

  const onDraft = (e: React.ChangeEvent<HTMLInputElement>) => setDraft(e.target.value);

  const openMenu = (e: React.MouseEvent, tile: Tile) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, tile });
  };

  const closeMenu = () => setMenu(null);

  const menuItems = (tile: Tile) => [
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

  const cardHandlers: TileCardHandlers = {
    onMenu: openMenu,
    onSelect: (id) => onFocusTile(id),
    onZoom: (id) => onFocusTile(id, true)
  };
  const frameHandlers = { onToggle: toggleFrame, onGoto: onFocusFrame };
  const renameField: RenameField = { value: draft, onCommit: commitRename, onChange: onDraft, onKeyDown: onRenameKey };

  const card = (tile: Tile, flat: boolean) => {
    const info: CardInfo = {
      now,
      query,
      status: statusOf(tile),
      activity: activity.get(tile.id),
      frame: flat ? frameOf.get(tile.id) : undefined
    };
    return (
      <TileCard
        key={tile.id}
        tile={tile}
        info={info}
        handlers={cardHandlers}
        active={tile.id === activeTile}
        rename={renaming === tile.id ? renameField : null}
      />
    );
  };

  const recency = (a: Tile, b: Tile) => (activity.get(b.id)?.at ?? 0) - (activity.get(a.id)?.at ?? 0);
  const urgency = (a: Tile, b: Tile) => STATUS_RANK[statusOf(a) ?? 'done'] - STATUS_RANK[statusOf(b) ?? 'done'] || recency(a, b);

  const renderFrames = () => {
    const looseCards = loose.filter(matches);
    if (query && !tiles.some(matches)) return <div className={styles.empty}>No tiles match</div>;
    return (
      <>
        {frames.map((frame) => {
          const children = (members.get(frame.id) ?? []).filter(matches);
          if (query && !children.length) return null;
          const statuses = children.map(statusOf).filter((s): s is TileStatus => s !== null);
          return (
            <FrameGroup
              key={frame.id}
              frame={frame}
              statuses={statuses}
              count={children.length}
              handlers={frameHandlers}
              open={Boolean(query) || !collapsed.has(frame.id)}
            >
              {children.map((tile) => card(tile, false))}
            </FrameGroup>
          );
        })}
        {looseCards.length > 0 && (
          <>
            {frames.length > 0 && <div className={styles.section}>No frame</div>}
            <div className={styles.cards}>{looseCards.map((tile) => card(tile, false))}</div>
          </>
        )}
      </>
    );
  };

  const renderFlat = () => {
    const pool = scope === 'attention' ? tiles.filter((t) => statusOf(t)) : tiles;
    const sorted = pool.filter(matches).sort(scope === 'attention' ? urgency : recency);
    if (!tiles.length) return null;
    if (!sorted.length) return <div className={styles.empty}>{scope === 'attention' ? 'Nothing needs you' : 'No tiles match'}</div>;
    return <div className={styles.cards}>{sorted.map((tile) => card(tile, true))}</div>;
  };

  const scopeLabel = (s: Scope) => {
    if (s === 'frames') return 'Frames';
    if (s === 'recent') return 'Recent';
    return attentionCount ? `Attention ${attentionCount}` : 'Attention';
  };

  return (
    <div className={styles.tab}>
      <div className={styles.bar}>
        <div className={styles.scopes} role="group" aria-label="Group tiles">
          {SCOPES.map((s) => (
            <button key={s} className={styles.scope} onClick={() => pickScope(s)} aria-pressed={scope === s} data-active={scope === s || undefined}>
              {scopeLabel(s)}
            </button>
          ))}
        </div>
        <button
          className={styles.compact}
          onClick={toggleCompact}
          aria-pressed={compact}
          aria-label="Compact cards"
          data-tooltip="Compact cards"
          data-tooltip-place="bottom"
          data-active={compact || undefined}
        >
          <AlignJustify size={14} strokeWidth={2} />
        </button>
      </div>
      <div className={styles.list} data-compact={compact || undefined}>
        {scope === 'frames' ? renderFrames() : renderFlat()}
        {tiles.length === 0 && <div className={styles.empty}>No tiles on canvas</div>}
      </div>
      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu} items={menuItems(menu.tile)} />}
    </div>
  );
};

export default TilesTab;
