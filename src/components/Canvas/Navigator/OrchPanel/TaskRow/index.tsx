import type { OpenDiffOpts, ShowWorkerOpts } from '../types';
import type { Task, Dispatch, Run } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { Check, ChevronRight, ExternalLink, X } from 'lucide-react';

import { formatDuration, ACTIVE_DISPATCH_STATUSES } from '~/usecase/util/orch';

import styles from './styles.module.scss';

interface TaskRowProps {
  task: Task;
  dispatches: Dispatch[];
  run: Run;
  open: boolean;
  onToggle: () => void;
  onOpenDiff: (root: string, file: string, opts: OpenDiffOpts) => void;
  onShowWorker: (opts: ShowWorkerOpts) => void;
}

const TaskRow = ({ task, dispatches, run, open, onToggle, onOpenDiff, onShowWorker }: TaskRowProps): React.ReactElement => {
  const taskDispatches = dispatches
    .filter((d) => d.taskId === task.id)
    .sort((a, b) => b.attempt - a.attempt);

  const openFile = (dispatch: Dispatch, file: string) => {
    if (dispatch.worktree && !dispatch.headSha) {
      onOpenDiff(dispatch.worktree, file, { base: run.baseSha });
    } else {
      onOpenDiff(run.repo, file, { commit: dispatch.headSha });
    }
  };

  return (
    <>
      <div className={styles.taskRow} data-open={open || undefined} onClick={onToggle}>
        <ChevronRight size={12} strokeWidth={2} className={styles.caret} data-open={open || undefined} />
        <span className={styles.statusDot} data-status={task.status} />
        <span className={styles.taskId}>{task.id}</span>
        <span className={styles.taskTitle}>{task.title}</span>
        {task.deps.length > 0 && (
          <span className={styles.taskDeps}>
            {task.deps.map((dep) => (
              <span key={dep} className={styles.depChip}>{dep}</span>
            ))}
          </span>
        )}
        {task.attempts > 1 && <span className={styles.taskAttempts}>x{task.attempts}</span>}
      </div>
      {open && taskDispatches.length > 0 && (
        <div className={styles.taskBody}>
          {taskDispatches.map((dispatch) => {
            const label = [dispatch.profile, dispatch.model].filter(Boolean).join(':') || dispatch.id;
            const duration = formatDuration(dispatch.startedAt, dispatch.endedAt);
            const verifications = dispatch.verifications ?? [];
            const showWorker = dispatch.tileId && ACTIVE_DISPATCH_STATUSES.has(dispatch.status);
            const handleShowWorker = showWorker
              ? () => onShowWorker({
                  tileId: dispatch.tileId!,
                  cwd: dispatch.worktree ?? '',
                  group: { id: run.id, title: run.title }
                })
              : undefined;

            return (
              <div key={dispatch.id} className={styles.dispatchRow}>
                <div className={styles.dispatchHead}>
                  <span className={styles.statusDot} data-status={dispatch.status} />
                  <span className={styles.dispatchLabel}>
                    #{dispatch.attempt} {label}
                  </span>
                  <span className={styles.dispatchMeta}>{duration}</span>
                  {dispatch.costUsd != null ? (
                    <span className={styles.dispatchMeta}>${dispatch.costUsd.toFixed(4)}</span>
                  ) : (
                    <span className={styles.dispatchMeta}>cost unknown</span>
                  )}
                </div>
                {verifications.length > 0 && (
                  <div className={styles.verifications}>
                    {verifications.map((v, i) => {
                      const pass = v.exitCode === 0 && !v.timedOut;
                      return (
                        <div key={i} className={styles.verifyRow}>
                          {pass ? (
                            <Check size={11} strokeWidth={2.5} className={styles.passIcon} />
                          ) : (
                            <X size={11} strokeWidth={2.5} className={styles.failIcon} />
                          )}
                          <span className={styles.verifyCmd}>{v.command}</span>
                          {v.exitCode != null && <span className={styles.dispatchMeta}>{v.exitCode}</span>}
                        </div>
                      );
                    })}
                  </div>
                )}
                {dispatch.filesModified.length > 0 && (
                  <div className={styles.fileList}>
                    {dispatch.filesModified.map((file) => {
                      const handleClick = () => openFile(dispatch, file);
                      return (
                        <button key={file} className={styles.fileRow} onClick={handleClick} type="button">
                          {file}
                        </button>
                      );
                    })}
                  </div>
                )}
                {showWorker && (
                  <div className={styles.dispatchActions}>
                    <button className={styles.workerBtn} onClick={handleShowWorker} type="button">
                      <ExternalLink size={11} strokeWidth={1.75} />
                      Show worker
                    </button>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </>
  );
};

export default TaskRow;
