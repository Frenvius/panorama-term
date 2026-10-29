import React from 'react';
import type { LucideIcon } from 'lucide-react';
import { Code, Globe, Image, Network, FileCode, GitBranch, StickyNote, SquareTerminal } from 'lucide-react';

import type { TileStatus } from '~/usecase/util/tileStatus';
import type { Frame, Tile } from '~/domain/interfaces/canvas.interface';
import type { TileActivity } from '~/usecase/util/tileActivity';
import type { TileType } from '~/domain/interfaces/workspace.interface';
import { elapsed } from '~/usecase/util/tileStatus';
import { timeAgo } from '~/usecase/util/agentHistory';
import { notePreview } from '~/usecase/util/noteMeta';
import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { tileLabel, folderName } from '~/usecase/util/title';
import { PiLogo, KimiLogo, CodexLogo, OpenCodeLogo, AntigravityLogo, GenericAgentLogo } from '~/components/commons/AgentIcons';

import styles from './styles.module.scss';

const TYPE_ICON: Record<TileType, { Icon: LucideIcon; color: string; name: string }> = {
  code: { Icon: Code, color: '#7a8aab', name: 'Code' },
  term: { Icon: SquareTerminal, color: '#7aab6e', name: 'Terminal' },
  note: { Icon: StickyNote, color: '#8a7aab', name: 'Note' },
  image: { Icon: Image, color: '#c07a6e', name: 'Image' },
  graph: { Icon: Network, color: '#c8a35a', name: 'Graph' },
  editor: { Icon: FileCode, color: '#6ea8ab', name: 'Editor' },
  browser: { Icon: Globe, color: '#5c9bcf', name: 'Browser' }
};

const AGENT: Record<string, { Logo: React.ComponentType<{ size?: number }>; name: string }> = {
  pi: { Logo: PiLogo, name: 'Pi' },
  kimi: { Logo: KimiLogo, name: 'Kimi' },
  codex: { Logo: CodexLogo, name: 'Codex' },
  claude: { Logo: ClaudeLogo, name: 'Claude' },
  generic: { Logo: GenericAgentLogo, name: 'Agent' },
  opencode: { Logo: OpenCodeLogo, name: 'OpenCode' },
  antigravity: { Logo: AntigravityLogo, name: 'Antigravity' }
};

const STATUS_LABEL: Record<TileStatus, string> = { waiting: 'waiting', working: 'working', done: 'done' };

export interface CardInfo {
  now: number;
  query: string;
  frame?: Frame;
  status: TileStatus | null;
  activity?: TileActivity;
}

export interface RenameField {
  value: string;
  onCommit: () => void;
  onChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
  onKeyDown: (e: React.KeyboardEvent<HTMLInputElement>) => void;
}

export interface TileCardHandlers {
  onSelect: (id: string) => void;
  onZoom: (id: string) => void;
  onMenu: (e: React.MouseEvent, tile: Tile) => void;
}

interface TileCardProps {
  tile: Tile;
  info: CardInfo;
  active: boolean;
  rename: RenameField | null;
  handlers: TileCardHandlers;
}

const escapeRegExp = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

const highlight = (text: string, query: string): React.ReactNode => {
  const words = query.split(/\s+/).filter((w) => w.length > 1);
  if (!words.length) return text;
  const parts = text.split(new RegExp(`(${words.map(escapeRegExp).join('|')})`, 'gi'));
  return parts.map((part, i) => (i % 2 ? <mark key={i}>{part}</mark> : part));
};

const stopClick = (e: React.MouseEvent) => e.stopPropagation();

const TileCard = ({ tile, info, active, rename, handlers }: TileCardProps) => {
  const { now, query, frame, status, activity } = info;
  const agent = activity?.agent ? (AGENT[activity.agent] ?? AGENT.generic) : null;
  const type = TYPE_ICON[tile.type] ?? TYPE_ICON.term;
  const snippet = activity?.snippet;
  const fallback = tile.type === 'note' ? notePreview(tile.content) : tile.url || tile.filePath || '';
  const ago = activity?.at ? timeAgo(activity.at) : '';
  const where = frame ? null : folderName(tile.cwd);

  const select = () => handlers.onSelect(tile.id);
  const zoom = () => handlers.onZoom(tile.id);
  const openMenu = (e: React.MouseEvent) => handlers.onMenu(e, tile);

  const statusText = status === 'working' && activity?.busySince ? elapsed(activity.busySince, now) : status && STATUS_LABEL[status];

  const renderSnippet = () => {
    if (snippet?.cmd) {
      return (
        <>
          <span className={styles.cmd}>$ {highlight(snippet.cmd, query)}</span>
          {snippet.text && <> &rarr; {highlight(snippet.text, query)}</>}
        </>
      );
    }
    if (snippet?.role) {
      return (
        <>
          <b>{snippet.role === 'user' ? 'You' : (agent?.name ?? 'Agent')}</b>: {highlight(snippet.text, query)}
        </>
      );
    }
    return highlight(fallback, query);
  };

  const hasSnippet = Boolean(snippet?.cmd || snippet?.text || fallback);

  return (
    <div
      className={styles.card}
      onClick={select}
      onDoubleClick={zoom}
      onContextMenu={openMenu}
      data-active={active || undefined}
    >
      <div className={styles.title}>
        {rename ? (
          <input
            autoFocus
            className={styles.rename}
            value={rename.value}
            onBlur={rename.onCommit}
            onChange={rename.onChange}
            onKeyDown={rename.onKeyDown}
            onClick={stopClick}
          />
        ) : (
          <span className={styles.name}>{highlight(tileLabel(tile), query)}</span>
        )}
        {status && (
          <span className={styles.status} data-status={status}>
            {statusText}
          </span>
        )}
      </div>
      {hasSnippet && <div className={styles.snippet}>{renderSnippet()}</div>}
      <div className={styles.meta}>
        <span className={styles.icon} data-tooltip={agent?.name ?? type.name}>
          {agent ? <agent.Logo size={12} /> : <type.Icon size={12} strokeWidth={1.75} style={{ color: type.color }} />}
        </span>
        {frame && (
          <span className={styles.frame}>
            <span className={styles.swatch} style={{ background: frame.color }} />
            {frame.title}
          </span>
        )}
        {where && <span className={styles.part}>{highlight(where, query)}</span>}
        {tile.branch && (
          <span className={styles.branch}>
            <GitBranch size={10} strokeWidth={2} />
            {highlight(tile.branch, query)}
          </span>
        )}
        {ago && <span className={styles.part}>{ago}</span>}
      </div>
    </div>
  );
};

export default TileCard;
