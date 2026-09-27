import React from 'react';
import { X, Plus } from 'lucide-react';

import styles from './styles.module.scss';

export interface NoteTabActions {
  add: () => void;
  close: (index: number) => void;
  select: (index: number) => void;
  rename: (index: number, name: string) => void;
}

interface NoteTabsProps {
  current: number;
  raised: boolean;
  actions: NoteTabActions;
  style: React.CSSProperties;
  tabs: { name: string; empty: boolean }[];
}

const NoteTabs = ({ tabs, current, raised, style, actions }: NoteTabsProps) => {
  const [editing, setEditing] = React.useState<number | null>(null);
  const [draft, setDraft] = React.useState('');
  const [armed, setArmed] = React.useState<number | null>(null);
  const multi = tabs.length > 1;

  const stop = (e: React.PointerEvent) => e.stopPropagation();

  const startEdit = (i: number) => {
    setDraft(tabs[i].name);
    setEditing(i);
  };

  const commitEdit = () => {
    if (editing === null) return;
    const name = draft.trim();
    if (name && name !== tabs[editing].name) actions.rename(editing, name);
    setEditing(null);
  };

  const onEditKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') commitEdit();
    if (e.key === 'Escape') setEditing(null);
  };

  const requestClose = (i: number) => (e: React.MouseEvent) => {
    e.stopPropagation();
    if (tabs[i].empty || armed === i) {
      setArmed(null);
      actions.close(i);
      return;
    }
    setArmed(i);
  };

  const disarm = () => setArmed(null);

  return (
    <div className={raised ? `${styles.wrap} ${styles.raised}` : styles.wrap} style={style}>
      <div className={styles.row} role="tablist" onPointerDown={stop}>
        {multi &&
          tabs.map((t, i) => (
            <div
              key={i}
              role="tab"
              aria-selected={i === current}
              className={i === current ? `${styles.tab} ${styles.current}` : styles.tab}
              onClick={() => actions.select(i)}
              onDoubleClick={() => startEdit(i)}
              onMouseLeave={disarm}
              title={t.name}
            >
              {editing === i ? (
                <input
                  className={styles.input}
                  value={draft}
                  onBlur={commitEdit}
                  onChange={(e) => setDraft(e.target.value)}
                  onKeyDown={onEditKey}
                  autoFocus
                />
              ) : (
                <span className={styles.name}>{t.name}</span>
              )}
              {raised && i === current && editing !== i && (
                <button
                  className={armed === i ? `${styles.close} ${styles.armed}` : styles.close}
                  onClick={requestClose(i)}
                  aria-label={armed === i ? 'Confirm close tab' : 'Close tab'}
                >
                  <X size={10} strokeWidth={2.5} />
                </button>
              )}
            </div>
          ))}
        {raised && (
          <button className={`${styles.tab} ${styles.add}`} onClick={actions.add} aria-label="New tab">
            <Plus size={11} strokeWidth={2.5} />
          </button>
        )}
      </div>
    </div>
  );
};

export default NoteTabs;
