import React from 'react';
import { Workflow } from 'lucide-react';

import styles from './styles.module.scss';
import { requestOrchBoard } from '~/usecase/util/orchBoardRequest';
import { useOrchCoordinator } from '~/usecase/hooks/useOrchCoordinator';

interface OrchChipProps {
  tileId: string;
}

const OrchChip = ({ tileId }: OrchChipProps) => {
  const { run, label, needsYou } = useOrchCoordinator(tileId);
  const runId = run?.run.id;

  const openBoard = React.useCallback(() => {
    if (runId) requestOrchBoard({ runId, nearTileId: tileId });
  }, [runId, tileId]);

  if (!label) return null;

  return (
    <button type="button" data-orch-chip onClick={openBoard} className={`${styles.chip} ${needsYou ? styles.amber : ''}`}>
      <Workflow size={9} strokeWidth={2} />
      <span>{label}</span>
    </button>
  );
};

export default OrchChip;
