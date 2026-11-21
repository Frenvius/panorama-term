import type { Dispatch, Run, RunSummary, CostEntry } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { ChevronDown, Pause, Play, Plus, X } from 'lucide-react';

import styles from './styles.module.scss';
import { formatDuration, totalCost } from '~/usecase/util/orch';
import { setProjectLastBoard } from '~/usecase/util/orchBoardRequest';
import { setRunCoordinator, setRunStatus, updateRun } from '~/adapter/orch/orch.client';

interface BoardHeaderProps {
  run: Run;
  cwd: string;
  dispatches: Dispatch[];
  projectRuns: RunSummary[];
  onPatchOrchTile: (patch: { runId?: string; boardTitle?: string }) => void;
  onNewBoard: () => void;
  onChanged: () => void;
  coordinatorCostUsd?: number;
  costByRole?: Record<string, CostEntry>;
  costByModel?: Record<string, CostEntry>;
}

const BoardHeader = ({ run, cwd, dispatches, projectRuns, onPatchOrchTile, onNewBoard, onChanged, coordinatorCostUsd, costByRole, costByModel }: BoardHeaderProps) => {
  const [switcherOpen, setSwitcherOpen] = React.useState(false);
  const [renaming, setRenaming] = React.useState(false);
  const [renameValue, setRenameValue] = React.useState('');
  const [confirmClose, setConfirmClose] = React.useState(false);
  const [headerError, setHeaderError] = React.useState<string | null>(null);
  const [breakdownOpen, setBreakdownOpen] = React.useState(false);

  const headerMainRef = React.useRef<HTMLDivElement>(null);
  const metaRef = React.useRef<HTMLDivElement>(null);

  const canPauseResume = run.status === 'active' || run.status === 'paused';
  const cost = totalCost(dispatches);
  const elapsed = formatDuration(run.createdAt);

  const hasBreakdown =
    !!(costByRole && Object.keys(costByRole).length > 0) ||
    !!(costByModel && Object.keys(costByModel).length > 0) ||
    coordinatorCostUsd != null;

  const totalReported = costByRole ? Object.values(costByRole).reduce((s, e) => s + e.reported, 0) : 0;

  React.useEffect(() => {
    if (!switcherOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!headerMainRef.current?.contains(e.target as Node)) setSwitcherOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setSwitcherOpen(false);
    };
    window.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
    };
  }, [switcherOpen]);

  React.useEffect(() => {
    if (!breakdownOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!metaRef.current?.contains(e.target as Node)) setBreakdownOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setBreakdownOpen(false);
    };
    window.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
    };
  }, [breakdownOpen]);

  const handleSwitchTo = (id: string) => {
    setSwitcherOpen(false);
    onPatchOrchTile({ runId: id });
    setProjectLastBoard(cwd, id);
  };

  const handleNewBoardNav = () => {
    setSwitcherOpen(false);
    onNewBoard();
  };

  const handleRenameStart = () => {
    setRenameValue(run.title);
    setRenaming(true);
  };

  const handleRenameSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!renameValue.trim()) {
      setRenaming(false);
      return;
    }
    setHeaderError(null);
    updateRun(run.id, renameValue.trim())
      .then(onChanged)
      .catch((err) => setHeaderError(err instanceof Error ? err.message : String(err)))
      .finally(() => setRenaming(false));
  };

  const handleRenameKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') setRenaming(false);
  };

  const handlePauseResume = () => {
    const next = run.status === 'paused' ? 'active' : 'paused';
    setHeaderError(null);
    setRunStatus(run.id, next)
      .then(onChanged)
      .catch((err) => setHeaderError(err instanceof Error ? err.message : String(err)));
  };

  const handleCloseClick = () => setConfirmClose(true);
  const handleCloseCancel = () => setConfirmClose(false);

  const handleCloseConfirm = () => {
    setHeaderError(null);
    setRunStatus(run.id, 'cancelled')
      .then(() => onPatchOrchTile({ runId: '' }))
      .catch((err) => setHeaderError(err instanceof Error ? err.message : String(err)));
  };

  const handleDetachCoordinator = () => {
    setHeaderError(null);
    setRunCoordinator(run.id, null)
      .then(onChanged)
      .catch((err) => setHeaderError(err instanceof Error ? err.message : String(err)));
  };

  const handleRenameChange = (e: React.ChangeEvent<HTMLInputElement>) => setRenameValue(e.target.value);
  const handleSwitcherToggle = () => setSwitcherOpen((v) => !v);
  const handleCostClick = () => setBreakdownOpen((v) => !v);

  return (
    <>
      <div className={styles.header}>
        <div className={styles.headerMain} ref={headerMainRef}>
          {renaming ? (
            <form onSubmit={handleRenameSubmit}>
              <input
                autoFocus
                type="text"
                value={renameValue}
                className={styles.renameInput}
                onChange={handleRenameChange}
                onBlur={handleRenameSubmit}
                onKeyDown={handleRenameKeyDown}
              />
            </form>
          ) : (
            <button type="button" className={styles.switcherBtn} onClick={handleSwitcherToggle}>
              <span className={styles.runStatusDot} data-status={run.status} />
              <span className={styles.switcherTitle}>{run.title}</span>
              <ChevronDown size={12} strokeWidth={2} className={styles.switcherCaret} />
            </button>
          )}

          {switcherOpen && (
            <div className={styles.switcherMenu}>
              {projectRuns.map((s) => (
                <button
                  key={s.run.id}
                  type="button"
                  className={styles.switcherItem}
                  data-active={s.run.id === run.id ? '' : undefined}
                  onClick={() => handleSwitchTo(s.run.id)}
                >
                  <span className={styles.switcherItemDot} data-status={s.run.status} />
                  <span className={styles.switcherItemTitle}>{s.run.title}</span>
                </button>
              ))}
              {projectRuns.length > 0 && <div className={styles.switcherDivider} />}
              <button type="button" className={styles.newBoardBtn} onClick={handleNewBoardNav}>
                <Plus size={12} strokeWidth={2} />
                New board
              </button>
            </div>
          )}

          {run.coordinatorTile && !renaming && (
            <span className={styles.coordinatorLabel}>
              Coordinated
              <button type="button" title="Detach coordinator" className={styles.coordinatorDetach} onClick={handleDetachCoordinator}>
                <X size={10} strokeWidth={2} />
              </button>
            </span>
          )}
        </div>

        <div className={styles.runMeta} ref={metaRef}>
          {cost != null && (
            hasBreakdown
              ? <button type="button" className={styles.costBtn} onClick={handleCostClick}>${cost.toFixed(4)}</button>
              : <span>${cost.toFixed(4)}</span>
          )}
          <span>{elapsed}</span>
          {breakdownOpen && (
            <div className={styles.breakdown}>
              {costByRole && Object.keys(costByRole).length > 0 && (
                <div className={styles.breakdownSection}>
                  <span className={styles.breakdownLabel}>By role</span>
                  {Object.entries(costByRole).map(([role, e]) => (
                    <div key={role} className={styles.breakdownRow}>
                      <span className={styles.breakdownKey}>{role}</span>
                      <span className={styles.breakdownVal}>${e.usd.toFixed(4)}</span>
                      <span className={styles.breakdownSub}>{e.reported}/{e.dispatches}</span>
                    </div>
                  ))}
                </div>
              )}
              {costByModel && Object.keys(costByModel).length > 0 && (
                <div className={styles.breakdownSection}>
                  <span className={styles.breakdownLabel}>By model</span>
                  {Object.entries(costByModel).map(([model, e]) => (
                    <div key={model} className={styles.breakdownRow}>
                      <span className={styles.breakdownKey}>{model}</span>
                      <span className={styles.breakdownVal}>${e.usd.toFixed(4)}</span>
                      <span className={styles.breakdownSub}>{e.reported}/{e.dispatches}</span>
                    </div>
                  ))}
                </div>
              )}
              {coordinatorCostUsd != null && (
                <div className={styles.breakdownSection}>
                  <div className={styles.breakdownRow}>
                    <span className={styles.breakdownKey}>Coordinator</span>
                    <span className={styles.breakdownVal}>${coordinatorCostUsd.toFixed(4)}</span>
                  </div>
                </div>
              )}
              {dispatches.length > 0 && (
                <div className={styles.breakdownCoverage}>
                  {totalReported}/{dispatches.length} dispatches reported
                </div>
              )}
              <div className={styles.breakdownNote}>Reported by agents (API-equivalent), not billed on subscriptions.</div>
            </div>
          )}
        </div>

        {!confirmClose && (
          <div className={styles.headerActions}>
            {!renaming && (
              <button type="button" className={styles.headerBtn} title="Rename" onClick={handleRenameStart}>
                Rename
              </button>
            )}
            {canPauseResume && (
              <button type="button" className={styles.headerBtn} onClick={handlePauseResume}>
                {run.status === 'paused' ? (
                  <><Play size={11} strokeWidth={2} />Resume</>
                ) : (
                  <><Pause size={11} strokeWidth={2} />Pause</>
                )}
              </button>
            )}
            {canPauseResume && (
              <button
                type="button"
                title="Close board (cancels queued and running tasks)"
                className={`${styles.headerBtn} ${styles.headerBtnDanger}`}
                onClick={handleCloseClick}
              >
                <X size={11} strokeWidth={2} />
                Close
              </button>
            )}
          </div>
        )}

        {confirmClose && (
          <div className={styles.confirmClose}>
            <span className={styles.confirmMsg}>Close board?</span>
            <button type="button" className={`${styles.headerBtn} ${styles.headerBtnDanger}`} onClick={handleCloseConfirm}>Yes</button>
            <button type="button" className={styles.headerBtn} onClick={handleCloseCancel}>No</button>
          </div>
        )}
      </div>

      {headerError && <div className={styles.headerError}>{headerError}</div>}
    </>
  );
};

export default BoardHeader;
