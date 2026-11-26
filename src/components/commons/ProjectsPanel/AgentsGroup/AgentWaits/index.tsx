import type { AgentWait, PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';
import ReactDOM from 'react-dom';
import { X } from 'lucide-react';

import styles from './styles.module.scss';
import { showToast } from '~/usecase/util/toasts';
import { agentActions } from '~/usecase/hooks/useAgents';
import { clampToViewport } from '~/usecase/util/agentMessages';
import { formatLeft, splitWaits } from '~/usecase/util/agentWaits';
import { cancelWait, listWaits } from '~/adapter/agents/agents.client';

interface AgentWaitsProps {
  agent: PersistentAgent;
  anchorRect: DOMRect;
  onClose: () => void;
}

const POPOVER_SIZE = { width: 340, height: 420 };

const formatTime = (at: number): string => new Date(at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });

const closedLine = (w: AgentWait): string => {
  if (w.status === 'cancelled') return `cancelled${w.cancelledBy ? ` by ${w.cancelledBy}` : ''}`;
  if (!w.fired) return w.status;
  return `${formatTime(w.fired.at)} ${w.fired.summary}`;
};

const AgentWaits = ({ agent, anchorRect, onClose }: AgentWaitsProps) => {
  const [waits, setWaits] = React.useState<AgentWait[]>([]);
  const [cancelling, setCancelling] = React.useState<string | null>(null);
  const popRef = React.useRef<HTMLDivElement>(null);
  const openKey = (agent.waits ?? []).map((w) => w.id).join(',');

  const load = React.useCallback(() => {
    listWaits(agent.name)
      .then((r) => setWaits(r.waits))
      .catch((err: unknown) => showToast({ kind: 'error', message: err instanceof Error ? err.message : String(err) }));
  }, [agent.name]);

  React.useEffect(() => {
    load();
  }, [load, openKey]);

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

  const handleCancel = (id: string) => {
    setCancelling(id);
    cancelWait(id)
      .then(() => {
        load();
        agentActions.refresh();
      })
      .catch((err: unknown) => showToast({ kind: 'error', message: err instanceof Error ? err.message : String(err) }))
      .finally(() => setCancelling(null));
  };

  const { open, history } = splitWaits(waits);
  const now = Date.now();
  const style: React.CSSProperties = clampToViewport(
    anchorRect.right + 8,
    anchorRect.top,
    POPOVER_SIZE,
    { width: window.innerWidth, height: window.innerHeight }
  );

  return ReactDOM.createPortal(
    <div ref={popRef} className={styles.popover} style={style}>
      <div className={styles.header}>
        <span className={styles.title}>{agent.name} waits</span>
        <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
          <X size={13} strokeWidth={2} />
        </button>
      </div>
      <div className={styles.list}>
        <div className={styles.section}>Open</div>
        {open.length === 0 && <div className={styles.empty}>Not waiting on anything</div>}
        {open.map((w) => (
          <div key={w.id} className={styles.waitRow}>
            <div className={styles.waitBody}>
              <span className={styles.waitOn}>{w.on.join(' | ')}</span>
              <span className={styles.waitMeta}>
                {w.timeoutAt != null ? formatLeft(w.timeoutAt - now) : 'timer'}
                {w.note ? ` - ${w.note}` : ''}
              </span>
            </div>
            <button
              type="button"
              className={styles.cancelBtn}
              disabled={cancelling === w.id}
              onClick={() => handleCancel(w.id)}
            >
              Cancel
            </button>
          </div>
        ))}
        {history.length > 0 && <div className={styles.section}>Recent</div>}
        {history.map((w) => (
          <div key={w.id} className={styles.waitRow}>
            <div className={styles.waitBody}>
              <span className={styles.waitOn}>{w.on.join(' | ')}</span>
              <span className={styles.waitMeta}>{closedLine(w)}</span>
            </div>
          </div>
        ))}
      </div>
    </div>,
    document.body,
  );
};

export default AgentWaits;
