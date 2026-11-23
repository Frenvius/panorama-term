import type { Task, Limits, Dispatch } from '~/domain/interfaces/orch.interface';

import styles from './styles.module.scss';
import { decisionLine, candidateLabel, DEFAULT_MAX_OPERATIONAL_ATTEMPTS } from '~/usecase/util/orchRouting';

interface RoutingInfoProps {
  task: Task;
  limits: Limits;
  dispatches: Dispatch[];
  latest?: Dispatch;
}

const dispatchModel = (d: Dispatch): string =>
  d.decision ? candidateLabel(d.decision.effective) : [d.profile, d.model].filter(Boolean).join(':');

const RoutingInfo = ({ task, limits, dispatches, latest }: RoutingInfoProps) => {
  const history = dispatches.filter((d) => d.taskId === task.id).sort((a, b) => a.attempt - b.attempt);
  const decision = latest ? decisionLine(latest) : null;
  const maxOperational = limits.maxOperationalAttempts ?? DEFAULT_MAX_OPERATIONAL_ATTEMPTS;

  return (
    <div className={styles.root}>
      {decision && <div className={styles.decision}>{decision}</div>}
      <div className={styles.counters}>
        <span title="Retries after a failed review or verification">
          Quality {task.qualityAttempts ?? 0}/{limits.maxAttempts}
        </span>
        <span title="Retries after crashes, quota or model unavailability">
          Operational {task.operationalAttempts ?? 0}/{maxOperational}
        </span>
      </div>
      {history.length > 0 && (
        <ol className={styles.history}>
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
