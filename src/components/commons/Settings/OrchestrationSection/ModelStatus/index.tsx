import type { CandidateKey, CandidateEntry } from '~/domain/interfaces/orch.interface';

import Group from '~/components/commons/Settings/Group';
import Button from '~/components/commons/Settings/Button';
import { formatClock } from '~/usecase/util/orchRouting';
import styles from './styles.module.scss';

interface ModelStatusProps {
  candidates: CandidateEntry[];
  onClear: (key: CandidateKey) => void;
}

interface StatusRowProps {
  entry: CandidateEntry;
  onClear: (key: CandidateKey) => void;
}

const StatusRow = ({ entry, onClear }: StatusRowProps) => {
  const { key } = entry;
  const handleClear = () => onClear(key);

  return (
    <div className={styles.row}>
      <span className={styles.agent}>{key.agent}</span>
      <span className={styles.model}>{key.model ?? 'all models'}</span>
      <span className={styles.dim}>{key.account ?? 'default account'}</span>
      <span className={styles.state} data-state={entry.state}>{entry.state.replace(/_/g, ' ')}</span>
      <span className={styles.dim}>{entry.until != null ? `until ${formatClock(entry.until)}` : 'until cleared'}</span>
      <span className={styles.dim}>{entry.source}</span>
      <Button size="sm" onClick={handleClear}>Clear</Button>
    </div>
  );
};

const ModelStatus = ({ candidates, onClear }: ModelStatusProps) => (
  <Group label="Model status">
    {candidates.length === 0 && <div className={styles.empty}>All models available.</div>}
    {candidates.map((entry) => (
      <StatusRow
        key={`${entry.key.agent}:${entry.key.model ?? ''}:${entry.key.account ?? ''}`}
        entry={entry}
        onClear={onClear}
      />
    ))}
  </Group>
);

export default ModelStatus;
