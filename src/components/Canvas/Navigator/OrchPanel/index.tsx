import type { OrchPanelProps } from './types';

import React from 'react';

import RunRow from './RunRow';
import RunDetailView from './RunDetailView';
import { useOrchRuns } from '~/usecase/hooks/useOrchRuns';

import styles from './styles.module.scss';

const OrchPanel = ({ onOpenDiff, onShowWorker }: OrchPanelProps) => {
  const { runs, details, selectedRunId, error, selectRun } = useOrchRuns();

  const selectedDetail = selectedRunId ? (details.get(selectedRunId) ?? null) : null;
  const handleBack = React.useCallback(() => selectRun(null), [selectRun]);

  if (selectedRunId && selectedDetail) {
    return (
      <div className={styles.root}>
        <RunDetailView
          detail={selectedDetail}
          onBack={handleBack}
          onOpenDiff={onOpenDiff}
          onShowWorker={onShowWorker}
        />
      </div>
    );
  }

  if (selectedRunId && !selectedDetail) {
    return <div className={styles.root}><div className={styles.notice}>Loading...</div></div>;
  }

  const sorted = [...runs].sort((a, b) => b.run.createdAt - a.run.createdAt);

  return (
    <div className={styles.root}>
      {error && <div className={styles.error}>{error}</div>}
      {sorted.length === 0 && <div className={styles.notice}>No orchestration runs</div>}
      {sorted.length > 0 && (
        <div className={styles.runList}>
          {sorted.map((summary) => (
            <RunRow
              key={summary.run.id}
              summary={summary}
              active={summary.run.id === selectedRunId}
              onSelect={() => selectRun(summary.run.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
};

export default OrchPanel;
