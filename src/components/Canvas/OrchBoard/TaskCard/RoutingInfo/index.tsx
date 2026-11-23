import type { Task, Limits, Dispatch } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { ChevronRight } from 'lucide-react';

import styles from './styles.module.scss';
import {
  waitLabel,
  decisionLine,
  candidateLabel,
  DEFAULT_MAX_OPERATIONAL_ATTEMPTS
} from '~/usecase/util/orchRouting';

interface RoutingInfoProps {
  task: Task;
  limits: Limits;
  dispatches: Dispatch[];
  latest?: Dispatch;
}

const dispatchModel = (d: Dispatch): string =>
  d.decision ? candidateLabel(d.decision.effective) : [d.profile, d.model].filter(Boolean).join(':');

const RoutingInfo = ({ task, limits, dispatches, latest }: RoutingInfoProps) => {
  const [open, setOpen] = React.useState(false);

  const routed = task.routing === 1;
  const history = dispatches.filter((d) => d.taskId === task.id).sort((a, b) => a.attempt - b.attempt);
  const decision = latest ? decisionLine(latest) : null;
  const maxOperational = limits.maxOperationalAttempts ?? DEFAULT_MAX_OPERATIONAL_ATTEMPTS;
  const showHistory = routed && history.some((d) => d.decision || d.failureClass);

  const toggleHistory = (e: React.MouseEvent) => {
    e.stopPropagation();
    setOpen((v) => !v);
  };

  const stopPropagation = (e: React.MouseEvent) => e.stopPropagation();

  if (!routed && !task.wait) return null;

  return (
    <div className={styles.root}>
      {task.status === 'waiting' && task.wait && (
        <span className={styles.waitChip} data-kind={task.wait.kind} title={task.wait.reason}>
          {waitLabel(task.wait, Date.now())}
        </span>
      )}
      {decision && <div className={styles.decision}>{decision}</div>}
      {routed && (
        <div className={styles.counters}>
          <span className={styles.counter}>q {task.qualityAttempts ?? 0}/{limits.maxAttempts}</span>
          <span>-</span>
          <span className={styles.counter}>o {task.operationalAttempts ?? 0}/{maxOperational}</span>
          {showHistory && (
            <button type="button" className={styles.historyBtn} aria-expanded={open} onClick={toggleHistory}>
              <ChevronRight size={10} strokeWidth={2} className={styles.caret} data-open={open ? '' : undefined} />
              History
            </button>
          )}
        </div>
      )}
      {open && (
        <ol className={styles.history} onClick={stopPropagation}>
          {history.map((d) => (
            <li key={d.id} className={styles.historyRow}>
              <div className={styles.historyHead}>
                <span className={styles.historyAttempt}>#{d.attempt}</span>
                <span className={styles.historyTrigger}>{d.trigger ?? 'initial'}</span>
                <span className={styles.historyModel}>{dispatchModel(d)}</span>
                {d.failureClass && <span className={styles.historyFailure}>{d.failureClass.replace(/_/g, ' ')}</span>}
              </div>
              {d.decision?.reason && <div className={styles.historyDim}>{d.decision.reason}</div>}
              {d.decision?.skipped.map((s) => (
                <div key={`${candidateLabel(s.candidate)}:${s.candidate.account ?? ''}`} className={styles.historyDim}>
                  Skipped {candidateLabel(s.candidate)} ({s.state.replace(/_/g, ' ')})
                </div>
              ))}
            </li>
          ))}
        </ol>
      )}
    </div>
  );
};

export default RoutingInfo;
