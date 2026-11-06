import styles from './styles.module.scss';

interface CrashNoticeProps {
  title: string;
  action: string;
  onAction: () => void;
}

const CrashNotice = ({ title, action, onAction }: CrashNoticeProps) => (
  <div className={styles.notice} role="alert">
    <span className={styles.title}>{title}</span>
    <span className={styles.hint}>Details in ui-errors.log</span>
    <button type="button" className={styles.action} onClick={onAction}>
      {action}
    </button>
  </div>
);

export default CrashNotice;
