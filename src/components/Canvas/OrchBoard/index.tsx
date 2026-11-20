import type { Task, RunDetail } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { Plus, RotateCcw } from 'lucide-react';

import TaskCard from './TaskCard';
import TaskForm from './TaskForm';
import GateBlock from './GateBlock';
import BoardPicker from './BoardPicker';
import BoardHeader from './BoardHeader';
import styles from './styles.module.scss';
import { normalizeRepo } from '~/usecase/util/orch';
import { watchOrchEvents } from '~/adapter/ui/ui.client';
import { showRun, resolveGate } from '~/adapter/orch/orch.client';
import { useOrchCoordinator } from '~/usecase/hooks/useOrchCoordinator';
import { setProjectLastBoard } from '~/usecase/util/orchBoardRequest';
import { type ColumnId, COLUMN_LABEL, COLUMNS, taskColumn } from './columns';

interface OrchBoardProps {
  runId: string;
  tileId: string;
  cwd: string;
  onPatchRunId: (runId: string) => void;
}

const OrchBoard = ({ runId, tileId, cwd, onPatchRunId }: OrchBoardProps) => {
  const [detail, setDetail] = React.useState<RunDetail | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [resolvingGate, setResolvingGate] = React.useState<string | null>(null);
  const [gateError, setGateError] = React.useState<string | null>(null);
  const [addingTask, setAddingTask] = React.useState(false);

  const { allRuns } = useOrchCoordinator(tileId);
  const normCwd = normalizeRepo(cwd);
  const projectRuns = normCwd
    ? allRuns.filter(
        (s) => normalizeRepo(s.run.repo) === normCwd && s.run.status !== 'completed' && s.run.status !== 'cancelled'
      )
    : [];

  const fetchDetail = React.useCallback(() => {
    if (!runId) return;
    showRun(runId)
      .then(({ ok: _ok, ...d }) => {
        setDetail(d as RunDetail);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, [runId]);

  React.useEffect(() => {
    if (!runId) {
      setDetail(null);
      return;
    }
    setProjectLastBoard(cwd, runId);
    fetchDetail();
    return watchOrchEvents(
      (changedId) => {
        if (!changedId || changedId === runId) fetchDetail();
      },
      fetchDetail
    );
  }, [runId, cwd, fetchDetail]);

  const handleResolve = (gateId: string, resolution: string) => {
    if (!detail) return;
    setResolvingGate(gateId);
    setGateError(null);
    resolveGate({ runId, gateId, resolution })
      .then(fetchDetail)
      .catch((err) => setGateError(err instanceof Error ? err.message : String(err)))
      .finally(() => setResolvingGate(null));
  };

  const handleAddTaskOpen = () => setAddingTask(true);
  const handleAddTaskClose = () => setAddingTask(false);

  if (!runId) {
    return (
      <div className={styles.root}>
        <BoardPicker cwd={cwd} projectRuns={projectRuns} onPatchRunId={onPatchRunId} />
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.root}>
        <div className={styles.errorMsg}>{error}</div>
        <button type="button" className={styles.retryBtn} onClick={fetchDetail}>
          <RotateCcw size={12} strokeWidth={2} />
          Retry
        </button>
      </div>
    );
  }

  if (!detail) {
    return <div className={styles.root}><div className={styles.loading}>Loading...</div></div>;
  }

  const { run, tasks, dispatches, gates } = detail;
  const openGates = gates.filter((g) => g.resolution == null);

  const byColumn: Record<ColumnId, Task[]> = {
    needsYou: [],
    queued: [],
    running: [],
    verifying: [],
    review: [],
    done: []
  };
  for (const task of tasks) {
    if (task.status !== 'cancelled') byColumn[taskColumn(task)].push(task);
  }
  const cancelled = tasks.filter((t) => t.status === 'cancelled');

  return (
    <div className={styles.root}>
      <BoardHeader
        run={run}
        cwd={cwd}
        dispatches={dispatches}
        projectRuns={projectRuns}
        onPatchRunId={onPatchRunId}
        onChanged={fetchDetail}
      />
      <div className={styles.board}>
        {COLUMNS.map((col) => {
          const colTasks = byColumn[col];
          const count = colTasks.length + (col === 'needsYou' ? openGates.length : 0) +
            (col === 'done' ? cancelled.length : 0);
          return (
            <div key={col} className={styles.column} data-col={col}>
              <div className={styles.colHeader}>
                <span className={styles.colDot} />
                <span className={styles.colLabel}>{COLUMN_LABEL[col]}</span>
                {count > 0 && <span className={styles.colCount}>{count}</span>}
                {col === 'queued' && !addingTask && (
                  <button type="button" className={styles.colAddBtn} onClick={handleAddTaskOpen}>
                    <Plus size={12} strokeWidth={2} />
                    New
                  </button>
                )}
              </div>
              <div className={styles.colBody}>
                {col === 'queued' && addingTask && <TaskForm runId={runId} onClose={handleAddTaskClose} />}
                {col === 'needsYou' && openGates.length > 0 && (
                  <>
                    {gateError && <div className={styles.gateError}>{gateError}</div>}
                    {openGates.map((gate) => (
                      <GateBlock
                        key={gate.id}
                        gate={gate}
                        resolving={resolvingGate}
                        onResolve={handleResolve}
                      />
                    ))}
                  </>
                )}
                {colTasks.map((task) => (
                  <TaskCard
                    key={task.id}
                    task={task}
                    runId={runId}
                    runRepo={run.repo}
                    runTitle={run.title}
                    runStatus={run.status}
                    dispatches={dispatches}
                    runBaseSha={run.baseSha}
                  />
                ))}
                {col === 'done' && cancelled.map((task) => (
                  <div key={task.id} className={styles.cancelled}>
                    <TaskCard
                      task={task}
                      runId={runId}
                      runRepo={run.repo}
                      runTitle={run.title}
                      runStatus={run.status}
                      dispatches={dispatches}
                      runBaseSha={run.baseSha}
                    />
                  </div>
                ))}
                {col === 'queued' && !addingTask && colTasks.length === 0 && (
                  <button type="button" className={styles.emptyAdd} onClick={handleAddTaskOpen}>
                    <Plus size={12} strokeWidth={2} />
                    Add a task
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
};

export default OrchBoard;
