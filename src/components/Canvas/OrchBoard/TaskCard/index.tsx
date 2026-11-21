import type { Task, Dispatch, RunStatus } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { Play, Square, ExternalLink, GitCompare, Pencil } from 'lucide-react';

import styles from './styles.module.scss';
import ReopenForm from './ReopenForm';
import EditForm from './EditForm';
import { latestDispatch } from '../columns';
import { cancelTask, updateTask, startWorker, workerStop, OrchError } from '~/adapter/orch/orch.client';
import { totalCost, formatDuration } from '~/usecase/util/orch';
import { requestOrchDiff, requestShowWorker } from '~/usecase/util/orchBoardRequest';

export interface TaskCardProps {
  task: Task;
  runId: string;
  runRepo: string;
  runTitle: string;
  runStatus: RunStatus;
  dispatches: Dispatch[];
  runBaseSha?: string;
}

type InlineView = 'reopen' | 'edit' | 'confirm-retry' | null;

const FLAGGED_STATUS: Partial<Record<Task['status'], string>> = {
  failed: 'failed',
  blocked: 'blocked',
  cancelled: 'cancelled'
};

const TaskCard = ({ task, dispatches, runId, runRepo, runTitle, runStatus, runBaseSha }: TaskCardProps) => {
  const dispatch = latestDispatch(task, dispatches);
  const [actionError, setActionError] = React.useState<string | null>(null);
  const [inlineView, setInlineView] = React.useState<InlineView>(null);
  const [acting, setActing] = React.useState(false);

  const profile = dispatch ? [dispatch.profile, dispatch.model].filter(Boolean).join(':') : task.profile;
  const elapsed = dispatch ? formatDuration(dispatch.startedAt, dispatch.endedAt) : undefined;
  const fileCount = dispatch?.filesModified.length ?? 0;
  const taskCost = totalCost(dispatches.filter((d) => d.taskId === task.id));
  const runPaused = runStatus === 'paused';
  const live = dispatch?.live ?? false;
  const starting = dispatch?.starting ?? false;
  const waiting = dispatch?.agentStatus === 'waiting' || (task.status === 'dispatched' && dispatch?.waiting === true);
  const flag = FLAGGED_STATUS[task.status];
  const failureReason = dispatch?.summary ? dispatch.summary.split('\n')[0] : null;

  const handleShowWorker = () => {
    if (!dispatch?.tileId) return;
    requestShowWorker({
      tileId: dispatch.tileId,
      cwd: dispatch.worktree ?? '',
      group: { id: runId, title: runTitle }
    });
  };

  const handleDiff = () => {
    if (!dispatch) return;
    const file = dispatch.filesModified[0];
    if (!file) return;
    if (dispatch.worktree && !dispatch.headSha) {
      requestOrchDiff({ root: dispatch.worktree, file, base: runBaseSha });
    } else {
      requestOrchDiff({ root: runRepo, file, commit: dispatch.headSha });
    }
  };

  const handleCardClick = () => {
    if (isClickable) handleShowWorker();
  };

  const handleShowBtnClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    handleShowWorker();
  };

  const handleDiffBtnClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    handleDiff();
  };

  const handleCancelBtnClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setActionError(null);
    cancelTask({ runId, taskId: task.id })
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)));
  };

  const handlePlayClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setActionError(null);
    setActing(true);
    startWorker({ runId, taskId: task.id, by: 'user' })
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)))
      .finally(() => setActing(false));
  };

  const handlePlayAgainClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setActionError(null);
    setActing(true);
    startWorker({ runId, taskId: task.id, by: 'user', retryOf: dispatch?.id })
      .catch((err) => {
        if (err instanceof OrchError && err.code === 'live_attempt') {
          setInlineView('confirm-retry');
        } else {
          setActionError(err instanceof Error ? err.message : String(err));
        }
      })
      .finally(() => setActing(false));
  };

  const handleStopAndRetry = (e: React.MouseEvent) => {
    e.stopPropagation();
    setActionError(null);
    setActing(true);
    startWorker({ runId, taskId: task.id, by: 'user', retryOf: dispatch?.id, stopLive: true })
      .then(() => setInlineView(null))
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)))
      .finally(() => setActing(false));
  };

  const handleShowLiveAttempt = (e: React.MouseEvent) => {
    e.stopPropagation();
    const liveAttempt = dispatches.find((d) => d.taskId === task.id && d.live);
    if (!liveAttempt?.tileId) return;
    requestShowWorker({
      tileId: liveAttempt.tileId,
      cwd: liveAttempt.worktree ?? '',
      group: { id: runId, title: runTitle }
    });
    setInlineView(null);
  };

  const handleStopWorker = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (!dispatch?.id) return;
    setActionError(null);
    workerStop({ runId, dispatchId: dispatch.id })
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)));
  };

  const handleEditClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setInlineView('edit');
  };

  const handleAcceptClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setActionError(null);
    setActing(true);
    updateTask({ runId, taskId: task.id, status: 'completed', by: 'user' })
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)))
      .finally(() => setActing(false));
  };

  const handleReopenClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    setInlineView('reopen');
  };

  const handleReopenSubmit = (note: string) => {
    setActionError(null);
    setActing(true);
    const spec = note ? `${task.spec}\n\nReviewer note: ${note}` : task.spec;
    updateTask({ runId, taskId: task.id, status: 'pending', spec, by: 'user' })
      .then(() => setInlineView(null))
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)))
      .finally(() => setActing(false));
  };

  const handleEditSubmit = (spec: string, prof: string, rev: boolean) => {
    setActionError(null);
    setActing(true);
    updateTask({ runId, taskId: task.id, spec, profile: prof || undefined, review: rev, by: 'user' })
      .then(() => setInlineView(null))
      .catch((err) => setActionError(err instanceof Error ? err.message : String(err)))
      .finally(() => setActing(false));
  };

  const handleInlineCancel = () => setInlineView(null);

  const isClickable = live && !!dispatch?.tileId;

  const showBtn = isClickable && (
    <button type="button" className={styles.cardBtn} onClick={handleShowBtnClick}>
      <ExternalLink size={11} strokeWidth={1.75} />
      Show
    </button>
  );

  return (
    <div className={styles.card} data-clickable={isClickable ? '' : undefined} onClick={handleCardClick}>
      <div className={styles.cardHead}>
        <span className={styles.cardTitle}>{task.title}</span>
        {waiting && <span className={styles.waitBadge}>needs approval</span>}
        {task.owner === 'user' && <span className={styles.ownerBadge}>you</span>}
      </div>

      {(flag || task.role || profile || elapsed || task.attempts > 1 || fileCount > 0 || taskCost != null) && (
        <div className={styles.cardMeta}>
          {flag && <span className={styles.flagChip} data-status={task.status}>{flag}</span>}
          {task.role && <span className={styles.roleChip}>{task.role}</span>}
          {profile && <span className={styles.metaChip}>{profile}</span>}
          {task.attempts > 1 && <span className={styles.metaChip}>attempt {task.attempts}</span>}
          {starting ? (
            <span className={styles.startingChip}>starting</span>
          ) : (
            live && <span className={styles.liveDot} />
          )}
          {elapsed && <span className={styles.metaDim}>{elapsed}</span>}
          {fileCount > 0 && <span className={styles.metaDim}>{fileCount} {fileCount === 1 ? 'file' : 'files'}</span>}
          {taskCost != null && <span className={styles.metaDim}>${taskCost.toFixed(4)}</span>}
        </div>
      )}

      {(task.status === 'failed' || task.status === 'blocked') && failureReason && (
        <div className={styles.failureReason} title={dispatch?.summary}>{failureReason}</div>
      )}

      {inlineView === 'reopen' && (
        <ReopenForm onSubmit={handleReopenSubmit} onCancel={handleInlineCancel} submitting={acting} />
      )}

      {inlineView === 'edit' && (
        <EditForm task={task} onSubmit={handleEditSubmit} onCancel={handleInlineCancel} submitting={acting} />
      )}

      {inlineView === 'confirm-retry' && (
        <div className={styles.retryConfirm}>
          <span className={styles.retryMsg}>An attempt is still running.</span>
          <div className={styles.retryActions}>
            <button type="button" className={`${styles.cardBtn} ${styles.stopBtn}`} disabled={acting} onClick={handleStopAndRetry}>
              Stop it and start new
            </button>
            <button type="button" className={styles.cardBtn} onClick={handleShowLiveAttempt}>
              Show it
            </button>
            <button type="button" className={styles.cardBtn} onClick={handleInlineCancel}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {inlineView === null && (
        <>
          {task.status === 'failed' && live && (
            <div className={styles.infoMsg}>failed, but the agent is still running</div>
          )}
          <div className={styles.cardActions}>
            {task.status === 'pending' && (
              <>
                <button
                  type="button"
                  className={`${styles.cardBtn} ${styles.playBtn}`}
                  disabled={acting || runPaused || !task.ready}
                  title={runPaused ? 'Run is paused' : !task.ready ? 'Dependencies not done' : 'Start this task'}
                  onClick={handlePlayClick}
                >
                  <Play size={11} strokeWidth={2} />
                  Play
                </button>
                {showBtn}
                <button type="button" className={styles.cardBtn} onClick={handleEditClick}>
                  <Pencil size={11} strokeWidth={1.75} />
                  Edit
                </button>
                <button type="button" className={`${styles.cardBtn} ${styles.stopBtn}`} onClick={handleCancelBtnClick}>
                  <Square size={11} strokeWidth={1.75} />
                  Cancel
                </button>
              </>
            )}

            {task.status === 'dispatched' && (
              <>
                {showBtn}
                <button type="button" className={`${styles.cardBtn} ${styles.stopBtn}`} onClick={handleCancelBtnClick}>
                  <Square size={11} strokeWidth={1.75} />
                  Stop
                </button>
              </>
            )}

            {task.status === 'verifying' && (
              <>
                {showBtn}
                {fileCount > 0 && (
                  <button type="button" className={styles.cardBtn} onClick={handleDiffBtnClick}>
                    <GitCompare size={11} strokeWidth={1.75} />
                    Diff
                  </button>
                )}
              </>
            )}

            {task.status === 'review' && (
              <>
                <button
                  type="button"
                  className={`${styles.cardBtn} ${styles.acceptBtn}`}
                  disabled={acting}
                  onClick={handleAcceptClick}
                >
                  Accept
                </button>
                <button type="button" className={styles.cardBtn} onClick={handleReopenClick}>
                  Reopen
                </button>
                {showBtn}
                {fileCount > 0 && (
                  <button type="button" className={styles.cardBtn} onClick={handleDiffBtnClick}>
                    <GitCompare size={11} strokeWidth={1.75} />
                    Diff
                  </button>
                )}
              </>
            )}

            {task.status === 'failed' && (
              <>
                {isClickable ? (
                  <>
                    {showBtn}
                    <button type="button" className={`${styles.cardBtn} ${styles.stopBtn}`} onClick={handleStopWorker}>
                      <Square size={11} strokeWidth={1.75} />
                      Stop
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      className={`${styles.cardBtn} ${styles.playBtn}`}
                      disabled={acting || runPaused}
                      title={runPaused ? 'Run is paused' : 'Retry this task'}
                      onClick={handlePlayAgainClick}
                    >
                      <Play size={11} strokeWidth={2} />
                      Play again
                    </button>
                    <button type="button" className={`${styles.cardBtn} ${styles.stopBtn}`} onClick={handleCancelBtnClick}>
                      <Square size={11} strokeWidth={1.75} />
                      Cancel
                    </button>
                  </>
                )}
              </>
            )}

            {task.status === 'blocked' && (
              <>
                {showBtn}
                <button type="button" className={`${styles.cardBtn} ${styles.stopBtn}`} onClick={handleCancelBtnClick}>
                  <Square size={11} strokeWidth={1.75} />
                  Cancel
                </button>
              </>
            )}

            {task.status === 'completed' && fileCount > 0 && (
              <button type="button" className={styles.cardBtn} onClick={handleDiffBtnClick}>
                <GitCompare size={11} strokeWidth={1.75} />
                Diff
              </button>
            )}
          </div>
        </>
      )}

      {actionError && <div className={styles.stopError}>{actionError}</div>}
    </div>
  );
};

export default TaskCard;
