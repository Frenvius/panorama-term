import React from 'react';

import styles from './styles.module.scss';

interface GroupProps {
  label?: string;
  action?: React.ReactNode;
  children: React.ReactNode;
}

const Group = ({ label, action, children }: GroupProps) => (
  <div className={styles.group}>
    {(label || action) && (
      <div className={label ? styles.header : styles.headerAction}>
        {label && <span className={styles.label}>{label}</span>}
        {action && <span className={styles.headerAction}>{action}</span>}
      </div>
    )}
    <div className={styles.card}>{children}</div>
  </div>
);

export default Group;
