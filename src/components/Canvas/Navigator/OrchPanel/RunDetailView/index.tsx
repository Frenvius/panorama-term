import type { OpenDiffOpts, ShowWorkerOpts } from '../types';
import type { RunDetail } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { ChevronRight } from 'lucide-react';

import TaskRow from '../TaskRow';
import { resolveGate } from '~/adapter/orch/orch.client';
import { totalCost, topoSort } from '~/usecase/util/orch';

import styles from './styles.module.scss';

interface RunDetailViewProps {
  detail: RunDetail;
  onBack: () => void;
  onOpenDiff: (root: string, file: string, opts: OpenDiffOpts) => void;
  onShowWorker: (opts: ShowWorkerOpts) => void;
}

const RunDetailView = ({ detail, onBack, onOpenDiff, onShowWorker }: RunDetailViewProps) => {
  const [openTasks, setOpenTasks] = React.useState<Set<string>>(() => new Set());
  const [resolvingGate, setResolvingGate] = React.useState<string | null>(null);
  const [gateError, setGateError] = React.useState<string | null>(null);

  const { run, tasks, dispatches, gates } = detail;
  const sortedTasks = React.useMemo(() => topoSort(tasks), [tasks]);
  const openGates = gates.filter((g) => g.resolution == null);
  const cost = totalCost(dispatches);
  const integrationBranch = `orch/${run.id}/integration`;

  const toggleTask = (id: string) => {
    setOpenTasks((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleResolve = (gateId: string, resolution: string) => {
    setResolvingGate(gateId);
    setGateError(null);
    resolveGate({ runId: run.id, gateId, resolution })
      .catch((err) => setGateError(err instanceof Error ? err.message : String(err)))
      .finally(() => setResolvingGate(null));
  };

  return (
    <div className={styles.detail}>
      <button className={styles.backBtn} onClick={onBack} type="button">
        <ChevronRight size={12} strokeWidth={2} style={{ transform: 'rotate(180deg)' }} />
        All runs
      </button>

      <div className={styles.detailHead}>
        <div className={styles.detailTitle}>{run.title}</div>
        <div className={styles.detailMeta}>
          <div className={styles.detailRow}>
            <span className={styles.detailLabel}>Status</span>
            <span className={styles.detailValue}>{run.status}</span>
          </div>
          <div className={styles.detailRow}>
            <span className={styles.detailLabel}>Repo</span>
            <span className={styles.detailValue}>{run.repo}</span>
          </div>
          <div className={styles.detailRow}>
            <span className={styles.detailLabel}>Branch</span>
            <span className={styles.detailValue}>{integrationBranch}</span>
          </div>
          {cost != null && (
            <div className={styles.detailRow}>
              <span className={styles.detailLabel}>Cost</span>
              <span className={styles.detailValue}>${cost.toFixed(4)} reported (API-equivalent)</span>
            </div>
          )}
        </div>
      </div>

      {openGates.length > 0 && (
        <div className={styles.section}>
          <div className={styles.sectionHead}>Gates</div>
          {gateError && <div className={styles.gateError}>{gateError}</div>}
          {openGates.map((gate) => (
            <div key={gate.id} className={styles.gateRow}>
              <div className={styles.gateQuestion}>{gate.question}</div>
              <div className={styles.gateOptions}>
                {gate.options.map((opt) => {
                  const handleClick = () => handleResolve(gate.id, opt);
                  return (
                    <button
                      key={opt}
                      className={styles.gateBtn}
                      onClick={handleClick}
                      disabled={resolvingGate === gate.id}
                      type="button"
                    >
                      {opt}
                    </button>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      )}

      <div className={styles.section}>
        <div className={styles.sectionHead}>Tasks</div>
        {sortedTasks.length === 0 && <div className={styles.notice}>No tasks</div>}
        {sortedTasks.map((task) => (
          <TaskRow
            key={task.id}
            task={task}
            dispatches={dispatches}
            run={run}
            open={openTasks.has(task.id)}
            onToggle={() => toggleTask(task.id)}
            onOpenDiff={onOpenDiff}
            onShowWorker={onShowWorker}
          />
        ))}
      </div>
    </div>
  );
};

export default RunDetailView;
