import type { MetricsRow, FailureClass } from '~/domain/interfaces/orch.interface';

import Group from '~/components/commons/Settings/Group';
import { formatDuration } from '~/usecase/util/orch';
import styles from './styles.module.scss';

interface ModelHistoryProps {
  rows: MetricsRow[];
}

const operationalTotal = (failures: Partial<Record<FailureClass, number>>): number =>
  Object.values(failures).reduce((sum, n) => sum + (n ?? 0), 0);

const median = (ms: number | null): string => (ms == null ? '-' : formatDuration(0, ms));

const ModelHistory = ({ rows }: ModelHistoryProps) => (
  <Group label="Model history">
    {rows.length === 0 ? (
      <div className={styles.empty}>No dispatches yet.</div>
    ) : (
      <table className={styles.table}>
        <thead>
          <tr>
            <th>Model</th>
            <th>Role</th>
            <th>Dispatches</th>
            <th>Verified</th>
            <th>Quality fails</th>
            <th>Operational fails</th>
            <th>Median time</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={`${row.agent}:${row.model ?? ''}:${row.role}`} data-low={row.lowSample ? '' : undefined}>
              <td className={styles.model}>
                {[row.agent, row.model].filter(Boolean).join(':')}
                {row.lowSample && <span className={styles.lowHint}>low sample</span>}
              </td>
              <td>{row.role}</td>
              <td>{row.dispatches}</td>
              <td>{row.verificationPassed}</td>
              <td>{row.qualityFailures}</td>
              <td>{operationalTotal(row.operationalFailures)}</td>
              <td>{median(row.medianDurationMs)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    )}
  </Group>
);

export default ModelHistory;
