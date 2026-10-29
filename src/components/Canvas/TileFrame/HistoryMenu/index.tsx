import React from 'react';
import ReactDOM from 'react-dom';
import { LoaderCircle } from 'lucide-react';

import { timeAgo } from '~/usecase/util/agentHistory';
import { agentHistory } from '~/adapter/claude/claude.client';

import type { ConversationEntry } from '~/domain/interfaces/claude.interface';

import styles from './styles.module.scss';

interface HistoryMenuProps {
  x: number;
  y: number;
  cwd: string;
  agent: string;
  onClose: () => void;
  onPick: (id: string) => void;
}

const EDGE = 8;

const HistoryMenu = ({ x, y, cwd, agent, onPick, onClose }: HistoryMenuProps) => {
  const rootRef = React.useRef<HTMLDivElement>(null);
  const [pos, setPos] = React.useState({ x, y });
  const [error, setError] = React.useState<string | null>(null);
  const [entries, setEntries] = React.useState<ConversationEntry[] | null>(null);

  React.useEffect(() => {
    let live = true;
    agentHistory(agent, cwd)
      .then((list) => live && setEntries(list))
      .catch((e) => live && setError(String(e)));
    return () => {
      live = false;
    };
  }, [agent, cwd]);

  React.useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const nx = Math.min(x, window.innerWidth - width - EDGE);
    const ny = Math.min(y, window.innerHeight - height - EDGE);
    setPos({ x: Math.max(EDGE, nx), y: Math.max(EDGE, ny) });
  }, [x, y, entries]);

  React.useEffect(() => {
    const onOutside = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('pointerdown', onOutside, true);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('pointerdown', onOutside, true);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [onClose]);

  const stop = (e: React.SyntheticEvent) => e.stopPropagation();

  const pick = (id: string) => {
    onPick(id);
    onClose();
  };

  const empty = entries?.length === 0;

  return ReactDOM.createPortal(
    <div ref={rootRef} data-ctx-menu className={styles.menu} style={{ top: pos.y, left: pos.x }} onPointerDown={stop} onWheel={stop}>
      <div className={styles.header}>
        <span>Conversations</span>
        {entries && entries.length > 0 && <span className={styles.count}>{entries.length}</span>}
      </div>
      {!entries && !error && (
        <div className={styles.status}>
          <LoaderCircle size={13} strokeWidth={2} className={styles.spin} />
          Loading
        </div>
      )}
      {error && <div className={styles.status}>{error}</div>}
      {empty && <div className={styles.status}>No conversations in this folder yet</div>}
      {entries && entries.length > 0 && (
        <div className={styles.list}>
          {entries.map((entry) => (
            <button key={entry.id} className={styles.item} onClick={() => pick(entry.id)} title={entry.title}>
              <span className={styles.title}>{entry.title}</span>
              <span className={styles.when}>{timeAgo(entry.updatedAt)}</span>
            </button>
          ))}
        </div>
      )}
    </div>,
    document.body
  );
};

export default HistoryMenu;
