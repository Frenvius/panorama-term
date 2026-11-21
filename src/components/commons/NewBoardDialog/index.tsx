import React from 'react';

import Dialog from '~/components/commons/Dialog';
import { createRun } from '~/adapter/orch/orch.client';
import { projectName } from '~/usecase/util/projectLayout';

import styles from './styles.module.scss';

interface NewBoardDialogProps {
  repo: string;
  onClose: () => void;
  onCreated: (runId: string, title: string) => void;
}

const NewBoardDialog = ({ repo, onClose, onCreated }: NewBoardDialogProps) => {
  const [title, setTitle] = React.useState(() => projectName(repo));
  const [creating, setCreating] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    inputRef.current?.select();
  }, []);

  const handleSubmit = () => {
    if (!title.trim() || creating) return;
    setCreating(true);
    setError(null);
    createRun({ repo, title: title.trim() })
      .then(({ run }) => onCreated(run.id, run.title))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err));
        setCreating(false);
      });
  };

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => setTitle(e.target.value);

  const footer = (
    <>
      {error && <span className={styles.error}>{error}</span>}
      <button type="button" className={styles.btn} onClick={onClose}>
        Cancel
      </button>
      <button
        type="submit"
        className={`${styles.btn} ${styles.primary}`}
        disabled={!title.trim() || creating}
      >
        {creating ? 'Creating...' : 'Create'}
      </button>
    </>
  );

  return (
    <Dialog title="New board" onClose={onClose} onSubmit={handleSubmit} footer={footer}>
      <input
        ref={inputRef}
        type="text"
        value={title}
        className={styles.input}
        placeholder="Board title"
        autoFocus
        onChange={handleChange}
      />
    </Dialog>
  );
};

export default NewBoardDialog;
