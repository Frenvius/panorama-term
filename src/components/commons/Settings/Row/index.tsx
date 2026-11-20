import React from 'react';

import styles from './styles.module.scss';

interface RowProps {
  settingId?: string;
  icon?: React.ReactNode;
  label: string;
  description?: string;
  children?: React.ReactNode;
  stacked?: boolean;
  disabled?: boolean;
}

const Row = ({ settingId, icon, label, description, children, stacked, disabled }: RowProps) => (
  <div
    data-setting={settingId}
    className={`${styles.row} ${stacked ? styles.stacked : ''} ${disabled ? styles.disabled : ''}`}
  >
    {icon && <span className={styles.icon}>{icon}</span>}
    <div className={styles.text}>
      <span className={styles.label}>{label}</span>
      {description && <span className={styles.description}>{description}</span>}
    </div>
    {children && (
      <div className={stacked ? styles.stackedControl : styles.control}>{children}</div>
    )}
  </div>
);

export default Row;
