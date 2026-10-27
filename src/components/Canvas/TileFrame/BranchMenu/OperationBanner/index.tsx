import { TriangleAlert } from 'lucide-react';

import type { RepoState, RepoOperationAction } from '~/domain/interfaces/git.interface';
import { canSkip, operationTitle } from '~/usecase/util/repoOperation';

import styles from './styles.module.scss';

interface OperationBannerProps {
  repo: RepoState;
  busy: boolean;
  onAction: (action: RepoOperationAction) => void;
}

const OperationBanner = ({ repo, busy, onAction }: OperationBannerProps) => {
  const proceed = () => onAction('continue');
  const skip = () => onAction('skip');
  const abort = () => onAction('abort');

  return (
    <div className={styles.banner} role="status">
      <div className={styles.head}>
        <TriangleAlert size={13} strokeWidth={2} className={styles.icon} />
        <span className={styles.title}>{operationTitle(repo)}</span>
      </div>
      <span className={styles.hint}>Resolve conflicts and stage them, then continue.</span>
      <div className={styles.actions}>
        <button type="button" className={styles.primary} onClick={proceed} disabled={busy}>
          Continue
        </button>
        {canSkip(repo) && (
          <button type="button" className={styles.button} onClick={skip} disabled={busy}>
            Skip
          </button>
        )}
        <button type="button" className={styles.danger} onClick={abort} disabled={busy}>
          Abort
        </button>
      </div>
    </div>
  );
};

export default OperationBanner;
