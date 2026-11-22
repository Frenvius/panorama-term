import type { ProviderUsage } from '~/domain/interfaces/usage.interface';

import UsageBar from '~/components/commons/StatusBar/UsageBar';
import UsageLogo from '~/components/commons/StatusBar/UsageLogo';
import { ERROR_TEXT, formatReset, WINDOW_TITLES, PROVIDER_NAMES } from '~/components/commons/StatusBar/constants';

import styles from './styles.module.scss';

interface UsageDetailsProps {
  usage: ProviderUsage;
}

const UsageDetails = ({ usage }: UsageDetailsProps) => (
  <div className={styles.panel}>
    <div className={styles.heading}>
      <UsageLogo provider={usage.provider} size={14} />
      {PROVIDER_NAMES[usage.provider]}
    </div>
    {usage.error && <div className={styles.error}>{ERROR_TEXT[usage.error] ?? usage.error}</div>}
    {usage.windows.map((w) => (
      <div key={w.key} className={styles.row}>
        <div className={styles.rowHead}>
          <span>{w.title ?? WINDOW_TITLES[w.key] ?? w.label}</span>
          <span className={styles.value}>{Math.round(w.used)}% used</span>
        </div>
        <UsageBar wide used={w.used} />
        {w.resetsAt && <div className={styles.reset}>{formatReset(w.resetsAt)}</div>}
      </div>
    ))}
  </div>
);

export default UsageDetails;
