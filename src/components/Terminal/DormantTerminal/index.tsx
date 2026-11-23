import { Play } from 'lucide-react';

import styles from './styles.module.scss';

interface DormantTerminalProps {
  cwd?: string;
  onStart: () => void;
}

const DormantTerminal = ({ cwd, onStart }: DormantTerminalProps) => (
  <div className={styles.dormant}>
    <span className={styles.icon}>
      <Play size={24} strokeWidth={1.5} />
    </span>
    <span className={styles.label}>Terminal stopped</span>
    {cwd && <span className={styles.cwd}>{cwd}</span>}
    <button type="button" className={styles.start} onClick={onStart}>
      Start terminal
    </button>
  </div>
);

export default DormantTerminal;
