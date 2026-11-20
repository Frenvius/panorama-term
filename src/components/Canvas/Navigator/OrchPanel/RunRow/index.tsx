import type { RunSummary } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { taskCountSummary } from '~/usecase/util/orch';

import styles from './styles.module.scss';

interface RunRowProps {
  summary: RunSummary;
  active: boolean;
  onSelect: () => void;
}

const RunRow = ({ summary, active, onSelect }: RunRowProps): React.ReactElement => (
  <div className={styles.runRow} data-active={active || undefined} onClick={onSelect}>
    <span className={styles.statusDot} data-status={summary.run.status} />
    <span className={styles.runTitle}>{summary.run.title}</span>
    <span className={styles.runCounts}>{taskCountSummary(summary.taskCounts)}</span>
  </div>
);

export default RunRow;
