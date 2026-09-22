import styles from './styles.module.scss';

interface UsageBarProps {
  used: number;
  wide?: boolean;
}

const UsageBar = ({ used, wide }: UsageBarProps) => {
  const level = used >= 80 ? styles.high : used >= 60 ? styles.mid : '';

  return (
    <span className={wide ? `${styles.track} ${styles.wide}` : styles.track}>
      <span style={{ width: `${used}%` }} className={`${styles.fill} ${level}`} />
    </span>
  );
};

export default UsageBar;
