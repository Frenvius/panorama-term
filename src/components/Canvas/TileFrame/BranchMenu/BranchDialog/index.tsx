import React from 'react';
import { GitBranch } from 'lucide-react';

import type { CommitInfo, BranchSnapshot } from '~/domain/interfaces/git.interface';
import Dialog from '~/components/commons/Dialog';
import { gitCompareWithCurrent } from '~/adapter/git/git.client';

import styles from './styles.module.scss';

export type DialogState =
  | { kind: 'create'; startPoint?: string }
  | { kind: 'rename'; branch: string }
  | { kind: 'confirm'; title: string; message: string; confirmLabel: string; run: () => Promise<BranchSnapshot> }
  | { kind: 'compare'; branch: string };

interface BranchDialogProps {
  state: DialogState;
  cwd: string;
  current: string;
  onClose: () => void;
  onCreate: (name: string, checkout: boolean, overwrite: boolean, startPoint?: string) => Promise<BranchSnapshot>;
  onRename: (oldName: string, newName: string) => Promise<BranchSnapshot>;
}

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const BranchDialog = ({ state, cwd, current, onClose, onCreate, onRename }: BranchDialogProps) => {
  const [name, setName] = React.useState(state.kind === 'rename' ? state.branch : '');
  const [checkout, setCheckout] = React.useState(true);
  const [overwrite, setOverwrite] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [commits, setCommits] = React.useState<CommitInfo[] | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    inputRef.current?.select();
  }, []);

  React.useEffect(() => {
    if (state.kind !== 'compare') return;
    let alive = true;
    gitCompareWithCurrent(cwd, state.branch)
      .then((list) => alive && setCommits(list))
      .catch((err: unknown) => alive && setError(message(err)));
    return () => {
      alive = false;
    };
  }, [state, cwd]);

  const trimmed = name.trim();
  const naming = state.kind === 'create' || state.kind === 'rename';
  const canSubmit = !busy && (!naming || trimmed.length > 0);

  const run = (): Promise<BranchSnapshot> | null => {
    if (state.kind === 'create') return onCreate(trimmed, checkout, overwrite, state.startPoint);
    if (state.kind === 'rename') return onRename(state.branch, trimmed);
    if (state.kind === 'confirm') return state.run();
    return null;
  };

  const submit = () => {
    const task = run();
    if (!task || !canSubmit) return;
    setBusy(true);
    setError(null);
    task
      .then(onClose)
      .catch((err: unknown) => {
        setError(message(err));
        setBusy(false);
      });
  };

  const onName = (e: React.ChangeEvent<HTMLInputElement>) => setName(e.target.value);
  const onCheckout = (e: React.ChangeEvent<HTMLInputElement>) => setCheckout(e.target.checked);
  const onOverwrite = (e: React.ChangeEvent<HTMLInputElement>) => setOverwrite(e.target.checked);

  const title =
    state.kind === 'create'
      ? state.startPoint
        ? `New branch from '${state.startPoint}'`
        : 'New branch'
      : state.kind === 'rename'
        ? 'Rename branch'
        : state.kind === 'confirm'
          ? state.title
          : `Compare '${state.branch}' with '${current}'`;

  const confirmLabel = state.kind === 'confirm' ? state.confirmLabel : state.kind === 'rename' ? 'Rename' : 'Create';

  const footer =
    state.kind === 'compare' ? (
      <button type="button" className={styles.btn} onClick={onClose}>
        Close
      </button>
    ) : (
      <>
        <button type="button" className={styles.btn} onClick={onClose}>
          Cancel
        </button>
        <button
          type="submit"
          className={state.kind === 'confirm' ? `${styles.btn} ${styles.danger}` : `${styles.btn} ${styles.primary}`}
          disabled={!canSubmit}
        >
          {busy ? 'Working...' : confirmLabel}
        </button>
      </>
    );

  return (
    <Dialog title={title} footer={footer} onClose={onClose} onSubmit={submit}>
      {naming && (
        <label className={styles.field}>
          <span className={styles.label}>Name</span>
          <input
            ref={inputRef}
            className={styles.input}
            value={name}
            onChange={onName}
            spellCheck={false}
            autoComplete="off"
            autoFocus
          />
        </label>
      )}

      {state.kind === 'create' && (
        <div className={styles.checks}>
          <label className={styles.check}>
            <input type="checkbox" checked={checkout} onChange={onCheckout} />
            Checkout branch
          </label>
          <label className={styles.check}>
            <input type="checkbox" checked={overwrite} onChange={onOverwrite} />
            Overwrite existing branch
          </label>
        </div>
      )}

      {state.kind === 'confirm' && <p className={styles.message}>{state.message}</p>}

      {state.kind === 'compare' && (
        <div className={styles.commits}>
          {!commits && !error && <span className={styles.hint}>Loading...</span>}
          {commits?.length === 0 && <span className={styles.hint}>No commits unique to this branch.</span>}
          {commits?.map((commit) => (
            <div key={commit.hash} className={styles.commit}>
              <GitBranch size={12} strokeWidth={2} className={styles.commitIcon} />
              <span className={styles.hash}>{commit.short}</span>
              <span className={styles.subject}>{commit.subject}</span>
              <span className={styles.meta}>
                {commit.author} - {commit.date}
              </span>
            </div>
          ))}
        </div>
      )}

      {error && <p className={styles.error}>{error}</p>}
    </Dialog>
  );
};

export default BranchDialog;
