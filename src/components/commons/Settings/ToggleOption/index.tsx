import { Check } from 'lucide-react';

import styles from '~/components/commons/Settings/styles.module.scss';

interface ToggleOptionProps {
  label: string;
  checked: boolean;
  description: string;
  onToggle: () => void;
}

const ToggleOption = ({ label, description, checked, onToggle }: ToggleOptionProps) => (
  <button
    type="button"
    onClick={onToggle}
    className={`${styles.option} ${checked ? styles.selected : ''}`}
  >
    <span className={styles.check}>{checked && <Check size={11} strokeWidth={3} />}</span>
    <span className={styles.optionText}>
      <span className={styles.optionLabel}>{label}</span>
      <span className={styles.optionDesc}>{description}</span>
    </span>
  </button>
);

export default ToggleOption;
