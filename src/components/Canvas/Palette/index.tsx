import React from 'react';
import type { LucideIcon } from 'lucide-react';
import { Code, Globe, Image, Search, Network, StickyNote, SquareTerminal } from 'lucide-react';

import type { Tile } from '~/domain/interfaces/canvas.interface';
import type { TileType } from '~/domain/interfaces/workspace.interface';
import { tileLabel } from '~/usecase/util/title';

import styles from './styles.module.scss';

const TYPE_ICON: Record<TileType, { Icon: LucideIcon; color: string }> = {
  term: { Icon: SquareTerminal, color: '#7aab6e' },
  note: { Icon: StickyNote, color: '#8a7aab' },
  code: { Icon: Code, color: '#7a8aab' },
  image: { Icon: Image, color: '#c07a6e' },
  graph: { Icon: Network, color: '#c8a35a' },
  browser: { Icon: Globe, color: '#5c9bcf' }
};

const tilePath = (tile: Tile): string =>
  tile.filePath ?? tile.folderPath ?? tile.cwd ?? tile.url ?? '';

const score = (text: string, query: string): number | null => {
  const hay = text.toLowerCase();
  let last = -1;
  let total = 0;
  for (const ch of query) {
    const at = hay.indexOf(ch, last + 1);
    if (at === -1) return null;
    const gap = at - last - 1;
    total += gap === 0 ? 15 : Math.max(2, 12 - gap);
    last = at;
  }
  if (hay.startsWith(query)) total += 20;
  else if (hay.includes(query)) total += 12;
  return total;
};

interface PaletteProps {
  tiles: Tile[];
  onSelect: (id: string) => void;
  onClose: () => void;
}

const Palette = ({ tiles, onSelect, onClose }: PaletteProps) => {
  const [query, setQuery] = React.useState('');
  const [cursor, setCursor] = React.useState(0);
  const inputRef = React.useRef<HTMLInputElement | null>(null);
  const listRef = React.useRef<HTMLDivElement | null>(null);

  React.useEffect(() => {
    inputRef.current?.focus({ preventScroll: true });
  }, []);

  const results = React.useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return tiles;
    return tiles
      .map((tile) => ({ tile, rank: score(`${tileLabel(tile)} ${tilePath(tile)}`, needle) }))
      .filter((entry): entry is { tile: Tile; rank: number } => entry.rank !== null)
      .sort((a, b) => b.rank - a.rank)
      .map((entry) => entry.tile);
  }, [tiles, query]);

  const at = Math.min(cursor, Math.max(results.length - 1, 0));

  React.useEffect(() => {
    listRef.current?.children[at]?.scrollIntoView({ block: 'nearest' });
  }, [at]);

  const pick = (id: string) => {
    onSelect(id);
    onClose();
  };

  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setCursor(0);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!results.length) return;
      const delta = e.key === 'ArrowDown' ? 1 : -1;
      setCursor((at + delta + results.length) % results.length);
      return;
    }
    if (e.key === 'Enter' && results[at]) {
      e.preventDefault();
      pick(results[at].id);
    }
  };

  const onBackdrop = (e: React.MouseEvent) => {
    e.preventDefault();
    onClose();
  };

  const stop = (e: React.MouseEvent) => e.stopPropagation();

  return (
    <div className={styles.backdrop} onMouseDown={onBackdrop}>
      <div className={styles.palette} onMouseDown={stop}>
        <div className={styles.inputRow}>
          <Search size={15} strokeWidth={1.75} className={styles.searchIcon} />
          <input
            ref={inputRef}
            value={query}
            className={styles.input}
            placeholder="Go to tile..."
            onChange={onQuery}
            onKeyDown={onKeyDown}
          />
        </div>
        <div ref={listRef} className={styles.results}>
          {!results.length && <div className={styles.empty}>No tiles found</div>}
          {results.map((tile, i) => {
            const { Icon, color } = TYPE_ICON[tile.type];
            const path = tilePath(tile);
            return (
              <button
                key={tile.id}
                type="button"
                className={styles.item}
                data-selected={i === at || undefined}
                onMouseMove={() => setCursor(i)}
                onClick={() => pick(tile.id)}
              >
                <Icon size={14} strokeWidth={1.75} style={{ color }} className={styles.icon} />
                <span className={styles.title}>{tileLabel(tile)}</span>
                {path && <span className={styles.path}>{path}</span>}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
};

export default Palette;
