import type { LucideIcon } from 'lucide-react';
import type { TileType } from '~/domain/interfaces/workspace.interface';
import type { CommandBarRow, CommandBarKind } from '~/domain/interfaces/commandBar.interface';

import React from 'react';
import {
  Zap,
  Code,
  Globe,
  Image,
  Network,
  PanelTop,
  FileCode,
  Settings,
  LayoutGrid,
  StickyNote,
  SquareDashed,
  SquareTerminal
} from 'lucide-react';

import styles from './styles.module.scss';

const TILE_ICON: Record<TileType, { Icon: LucideIcon; color: string }> = {
  term: { Icon: SquareTerminal, color: '#7aab6e' },
  note: { Icon: StickyNote, color: '#8a7aab' },
  code: { Icon: Code, color: '#7a8aab' },
  editor: { Icon: FileCode, color: '#6ea8ab' },
  image: { Icon: Image, color: '#c07a6e' },
  graph: { Icon: Network, color: '#c8a35a' },
  browser: { Icon: Globe, color: '#5c9bcf' }
};

const KIND_ICON: Record<Exclude<CommandBarKind, 'tile'>, LucideIcon> = {
  tab: PanelTop,
  command: Zap,
  setting: Settings,
  frame: SquareDashed,
  workspace: LayoutGrid
};

const iconOf = (row: CommandBarRow): { Icon: LucideIcon; color?: string } => {
  const { kind, tileType, color } = row.item;
  if (kind === 'tile') return TILE_ICON[tileType ?? 'term'];
  return { Icon: KIND_ICON[kind], color };
};

const highlight = (label: string, hits: number[]): React.ReactNode[] => {
  const marked = new Set(hits);
  const parts: React.ReactNode[] = [];
  let start = 0;
  for (let i = 1; i <= label.length; i++) {
    if (i < label.length && marked.has(i) === marked.has(start)) continue;
    const text = label.slice(start, i);
    parts.push(marked.has(start) ? <mark key={start} className={styles.hit}>{text}</mark> : text);
    start = i;
  }
  return parts;
};

interface RowProps {
  index: number;
  row: CommandBarRow;
  selected: boolean;
  onHover: (index: number) => void;
  onPick: (row: CommandBarRow) => void;
}

const Row = ({ row, index, selected, onHover, onPick }: RowProps) => {
  const { Icon, color } = iconOf(row);
  const { label, scope, hint, detail, alert } = row.item;

  return (
    <button
      type="button"
      role="option"
      data-index={index}
      className={styles.item}
      aria-selected={selected}
      data-selected={selected || undefined}
      onMouseMove={() => onHover(index)}
      onClick={() => onPick(row)}
    >
      <Icon size={14} strokeWidth={1.75} style={{ color }} className={styles.icon} />
      <span className={styles.title}>{highlight(label, row.hits)}</span>
      {alert && <span className={styles.alert} aria-label="Needs attention" />}
      {scope && <span className={styles.scope}>{scope}</span>}
      {hint ? <kbd className={styles.hint}>{hint}</kbd> : detail && <span className={styles.path}>{detail}</span>}
    </button>
  );
};

export default Row;
