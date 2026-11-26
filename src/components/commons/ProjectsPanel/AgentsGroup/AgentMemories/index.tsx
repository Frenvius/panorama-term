import type { AgentMemory, MemoryUsage, PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';
import ReactDOM from 'react-dom';
import { X } from 'lucide-react';

import styles from './styles.module.scss';
import { agentActions } from '~/usecase/hooks/useAgents';
import { clampToViewport } from '~/usecase/util/agentMessages';
import { usageBar, sortMemories, canMakeEssential } from '~/usecase/util/agentMemory';
import { editMemory, forgetMemory, listMemories, rememberMemory, setMemoryEssential } from '~/adapter/agents/agents.client';

interface AgentMemoriesProps {
  agent: PersistentAgent;
  anchorRect: DOMRect;
  onClose: () => void;
}

const POPOVER_SIZE = { width: 380, height: 520 };
const TEXT_MAX = 500;

const errorText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

const AgentMemories = ({ agent, anchorRect, onClose }: AgentMemoriesProps) => {
  const [memories, setMemories] = React.useState<AgentMemory[]>([]);
  const [usage, setUsage] = React.useState<MemoryUsage | null>(null);
  const [draft, setDraft] = React.useState('');
  const [essential, setEssential] = React.useState(false);
  const [editing, setEditing] = React.useState<{ id: string; text: string } | null>(null);
  const [confirmForget, setConfirmForget] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const popRef = React.useRef<HTMLDivElement>(null);
  const summaryKey = JSON.stringify(agent.memory ?? null);

  const load = React.useCallback(() => {
    listMemories(agent.name)
      .then((r) => {
        setMemories(sortMemories(r.memories));
        setUsage(r.usage);
      })
      .catch((err: unknown) => setError(errorText(err)));
  }, [agent.name]);

  React.useEffect(() => {
    load();
  }, [load, summaryKey]);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    const onOutside = (e: PointerEvent) => {
      if (!popRef.current?.contains(e.target as Node)) onClose();
    };
    document.addEventListener('keydown', onKey, true);
    document.addEventListener('pointerdown', onOutside, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      document.removeEventListener('pointerdown', onOutside, true);
    };
  }, [onClose]);

  const done = () => {
    setError(null);
    load();
    agentActions.refresh();
  };

  const fail = (err: unknown) => setError(errorText(err));

  const handleAdd = () => {
    if (!draft.trim()) return;
    rememberMemory(agent.name, draft, essential)
      .then(() => {
        setDraft('');
        setEssential(false);
        done();
      })
      .catch(fail);
  };

  const handleToggle = (m: AgentMemory) => {
    setMemoryEssential(agent.name, m.id, !m.essential).then(done).catch(fail);
  };

  const handleSaveEdit = () => {
    if (!editing) return;
    editMemory(agent.name, editing.id, editing.text)
      .then(() => {
        setEditing(null);
        done();
      })
      .catch(fail);
  };

  const handleForget = (id: string) => {
    setConfirmForget(null);
    forgetMemory(agent.name, id).then(done).catch(fail);
  };

  const style: React.CSSProperties = clampToViewport(anchorRect.right + 8, anchorRect.top, POPOVER_SIZE, {
    width: window.innerWidth,
    height: window.innerHeight
  });

  const bar = usage ? usageBar(usage) : null;

  return ReactDOM.createPortal(
    <div ref={popRef} className={styles.popover} style={style}>
      <div className={styles.header}>
        <span className={styles.title}>{agent.name} memories</span>
        <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
          <X size={13} strokeWidth={2} />
        </button>
      </div>
      {bar && (
        <div className={styles.usage}>
          <div className={styles.bar}>
            <div className={styles.barFill} data-full={bar.full} style={{ width: `${bar.pct}%` }} />
          </div>
          <span className={styles.meta}>{bar.entriesText} - {bar.bytesText}</span>
        </div>
      )}
      <div className={styles.list}>
        {memories.length === 0 && <div className={styles.empty}>No memories yet</div>}
        {memories.map((m) => (
          <div key={m.id} className={styles.item}>
            <div className={styles.itemTop}>
              <span className={styles.chip} data-kind={m.essential ? 'reported' : 'none'}>{m.essential ? 'essential' : 'searchable'}</span>
              <span className={styles.meta}>by {m.by}</span>
            </div>
            {editing?.id === m.id ? (
              <input
                className={styles.input}
                value={editing.text}
                maxLength={TEXT_MAX}
                onChange={(e) => setEditing({ id: m.id, text: e.target.value })}
              />
            ) : (
              <span className={styles.memText}>{m.text}</span>
            )}
            <div className={styles.actions}>
              {editing?.id === m.id ? (
                <>
                  <button type="button" className={styles.btn} onClick={() => setEditing(null)}>Cancel</button>
                  <button type="button" className={styles.primary} onClick={handleSaveEdit}>Save</button>
                </>
              ) : (
                <>
                  <button
                    type="button"
                    className={styles.btn}
                    disabled={usage ? !canMakeEssential(m, usage) : false}
                    title={usage && !canMakeEssential(m, usage) ? 'Essential notes are full' : undefined}
                    onClick={() => handleToggle(m)}
                  >
                    {m.essential ? 'Make searchable' : 'Make essential'}
                  </button>
                  <button type="button" className={styles.btn} onClick={() => setEditing({ id: m.id, text: m.text })}>Edit</button>
                  {confirmForget === m.id ? (
                    <button type="button" className={styles.danger} onClick={() => handleForget(m.id)}>Confirm forget</button>
                  ) : (
                    <button type="button" className={styles.btn} onClick={() => setConfirmForget(m.id)}>Forget</button>
                  )}
                </>
              )}
            </div>
          </div>
        ))}
      </div>
      <div className={styles.form}>
        <input
          className={styles.input}
          value={draft}
          maxLength={TEXT_MAX}
          placeholder="A fact or preference, one line"
          onChange={(e) => setDraft(e.target.value)}
        />
        <label className={styles.check}>
          <input type="checkbox" checked={essential} onChange={(e) => setEssential(e.target.checked)} />
          Essential (in every new session)
        </label>
        {error && <span className={styles.error}>{error}</span>}
        <div className={styles.actions}>
          <button type="button" className={styles.primary} disabled={!draft.trim()} onClick={handleAdd}>Remember</button>
        </div>
      </div>
    </div>,
    document.body,
  );
};

export default AgentMemories;
