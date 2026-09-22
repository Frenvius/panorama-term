import type { ProviderUsage } from '~/domain/interfaces/usage.interface';

import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { CodexLogo } from '~/components/commons/AgentIcons';
import UsageBar from '~/components/commons/StatusBar/UsageBar';
import { formatReset, WINDOW_TITLES, PROVIDER_NAMES } from '~/components/commons/StatusBar/constants';

import styles from './styles.module.scss';

interface UsageDetailsProps {
  usage: ProviderUsage;
}

const UsageDetails = ({ usage }: UsageDetailsProps) => (
  <div className={styles.panel}>
    <div className={styles.heading}>
      {usage.provider === 'claude' ? <ClaudeLogo size={14} /> : <CodexLogo size={14} />}
      {PROVIDER_NAMES[usage.provider]}
    </div>
    {usage.error && <div className={styles.error}>{usage.error === 'signed-out' ? 'Sign in to see usage' : usage.error}</div>}
    {usage.windows.map((w) => (
      <div key={w.key} className={styles.row}>
        <div className={styles.rowHead}>
          <span>{WINDOW_TITLES[w.key] ?? w.label}</span>
          <span className={styles.value}>{Math.round(w.used)}% used</span>
        </div>
        <UsageBar wide used={w.used} />
        {w.resetsAt && <div className={styles.reset}>{formatReset(w.resetsAt)}</div>}
      </div>
    ))}
  </div>
);

export default UsageDetails;
