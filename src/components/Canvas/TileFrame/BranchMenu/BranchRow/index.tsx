import React from 'react';
import { Star, Loader2, GitBranch } from 'lucide-react';

import type { BranchLeaf } from '~/domain/interfaces/git.interface';

import styles from './styles.module.scss';

export interface BranchRowState {
  dimmed: boolean;
  working: boolean;
  selected: boolean;
}

export interface BranchRowHandlers {
  onOpen: (leaf: BranchLeaf, rect: DOMRect) => void;
  onFavorite: (leaf: BranchLeaf) => void;
}

interface BranchRowProps {
  id: string;
  leaf: BranchLeaf;
  label: string;
  depth: number;
  matches: number[];
  state: BranchRowState;
  handlers: BranchRowHandlers;
}

const highlight = (label: string, matches: number[]): React.ReactNode[] => {
  const hits = new Set(matches);
  const parts: React.ReactNode[] = [];
  let run = '';
  let marked = false;
  const flush = (end: number) => {
    if (!run) return;
    parts.push(marked ? <mark key={end}>{run}</mark> : run);
    run = '';
  };
  for (let i = 0; i < label.length; i++) {
    if (hits.has(i) !== marked) {
      flush(i);
      marked = hits.has(i);
    }
    run += label[i];
  }
  flush(label.length);
  return parts;
};

const BranchRow = ({ id, leaf, label, depth, matches, state, handlers }: BranchRowProps) => {
  const track = [leaf.ahead > 0 && `+${leaf.ahead}`, leaf.behind > 0 && `-${leaf.behind}`].filter(Boolean).join(' ');

  const open = (e: React.MouseEvent) => {
    e.preventDefault();
    handlers.onOpen(leaf, (e.currentTarget as HTMLElement).getBoundingClientRect());
  };

  const favorite = (e: React.MouseEvent) => {
    e.stopPropagation();
    handlers.onFavorite(leaf);
  };

  return (
    <div
      className={styles.row}
      style={{ paddingLeft: 8 + depth * 12 }}
      onClick={open}
      onContextMenu={open}
      data-row-id={id}
      data-current={leaf.isCurrent || undefined}
      data-selected={state.selected || undefined}
      data-dimmed={state.dimmed || undefined}
    >
      {state.working ? (
        <span className={styles.star}>
          <Loader2 size={12} strokeWidth={2} className={styles.spinning} />
        </span>
      ) : (
        <button className={styles.star} onClick={favorite} aria-label="Toggle favorite" data-on={leaf.isFavorite || undefined}>
          {leaf.isFavorite ? <Star size={12} fill="currentColor" /> : <GitBranch size={12} strokeWidth={1.75} />}
        </button>
      )}
      <span className={styles.name}>{highlight(label, matches)}</span>
      {track && <span className={styles.track}>{track}</span>}
      {leaf.upstream && <span className={styles.upstream}>{leaf.upstream}</span>}
    </div>
  );
};

export default BranchRow;
