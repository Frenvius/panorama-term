import type { UsageWindow, ProviderUsage } from '~/domain/interfaces/usage.interface';

import React from 'react';

import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { CodexLogo } from '~/components/commons/AgentIcons';
import UsageBar from '~/components/commons/StatusBar/UsageBar';
import UsageDetails from '~/components/commons/StatusBar/UsageDetails';
import { formatLeft, CLOSE_DELAY } from '~/components/commons/StatusBar/constants';

import styles from './styles.module.scss';

interface UsageSegmentProps {
  usage: ProviderUsage;
}

const windowLabel = (w: UsageWindow) => (w.key === 'session' && w.resetsAt ? formatLeft(w.resetsAt - Date.now()) : w.label);

const UsageSegment = ({ usage }: UsageSegmentProps) => {
  const [open, setOpen] = React.useState(false);
  const timer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const tightest = usage.windows.reduce<UsageWindow | null>((max, w) => (!max || w.used > max.used ? w : max), null);

  const openDetails = () => {
    clearTimeout(timer.current);
    setOpen(true);
  };

  const closeDetails = () => {
    timer.current = setTimeout(() => setOpen(false), CLOSE_DELAY);
  };

  React.useEffect(() => () => clearTimeout(timer.current), []);

  return (
    <span className={styles.segment} onPointerEnter={openDetails} onPointerLeave={closeDetails}>
      {open && <UsageDetails usage={usage} />}
      {usage.provider === 'claude' ? <ClaudeLogo size={12} /> : <CodexLogo size={12} />}
      {tightest && <UsageBar used={tightest.used} />}
      {usage.windows.map((w, i) => (
        <React.Fragment key={w.key}>
          {i > 0 && <span className={styles.sep}>·</span>}
          <span className={styles.value}>
            {Math.round(w.used)}% {windowLabel(w)}
          </span>
        </React.Fragment>
      ))}
      {!tightest && <span className={styles.value}>--</span>}
    </span>
  );
};

export default UsageSegment;
