import styles from './styles.module.scss';

interface SwitchProps {
  checked: boolean;
  onChange: () => void;
  label: string;
  disabled?: boolean;
}

const Switch = ({ checked, onChange, label, disabled }: SwitchProps) => (
  <button
    type="button"
    role="switch"
    aria-checked={checked}
    aria-label={label}
    disabled={disabled}
    onClick={onChange}
    className={styles.switch}
  >
    <span className={styles.thumb} />
  </button>
);

export default Switch;
