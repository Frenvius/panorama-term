import type { RunSummary } from '~/domain/interfaces/orch.interface';

import React from 'react';

import styles from './styles.module.scss';
import { createRun } from '~/adapter/orch/orch.client';
import { setProjectLastBoard } from '~/usecase/util/orchBoardRequest';

interface BoardPickerProps {
  cwd: string;
  projectRuns: RunSummary[];
  onPatchRunId: (runId: string) => void;
}

const BoardPicker = ({ cwd, projectRuns, onPatchRunId }: BoardPickerProps) => {
  const [newBoardTitle, setNewBoardTitle] = React.useState('');
  const [creatingBoard, setCreatingBoard] = React.useState(false);
  const [createError, setCreateError] = React.useState<string | null>(null);

  const handleSwitchTo = (id: string) => {
    onPatchRunId(id);
    setProjectLastBoard(cwd, id);
  };

  const handleNewBoard = (e: React.FormEvent) => {
    e.preventDefault();
    if (!newBoardTitle.trim() || creatingBoard) return;
    setCreatingBoard(true);
    setCreateError(null);
    createRun({ repo: cwd, title: newBoardTitle.trim() })
      .then(({ run }) => {
        setNewBoardTitle('');
        onPatchRunId(run.id);
        setProjectLastBoard(cwd, run.id);
      })
      .catch((err) => setCreateError(err instanceof Error ? err.message : String(err)))
      .finally(() => setCreatingBoard(false));
  };

  const handleTitleChange = (e: React.ChangeEvent<HTMLInputElement>) => setNewBoardTitle(e.target.value);

  return (
    <div className={styles.picker}>
      {projectRuns.length > 0 && (
        <>
          <div className={styles.pickerTitle}>Boards for this project</div>
          {projectRuns.map((s) => (
            <button key={s.run.id} type="button" className={styles.pickerBoard} onClick={() => handleSwitchTo(s.run.id)}>
              <span className={styles.pickerBoardDot} data-status={s.run.status} />
              <span className={styles.pickerBoardTitle}>{s.run.title}</span>
            </button>
          ))}
        </>
      )}
      <div className={styles.pickerNewLabel}>New board</div>
      <form className={styles.pickerNewRow} onSubmit={handleNewBoard}>
        <input
          autoFocus
          type="text"
          value={newBoardTitle}
          placeholder="Board title"
          className={styles.pickerNewInput}
          onChange={handleTitleChange}
        />
        <button type="submit" className={styles.pickerNewSubmit} disabled={!newBoardTitle.trim() || creatingBoard}>
          {creatingBoard ? 'Creating...' : 'Create'}
        </button>
      </form>
      {createError && <div className={styles.gateError}>{createError}</div>}
    </div>
  );
};

export default BoardPicker;
