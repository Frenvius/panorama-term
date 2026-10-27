import React from 'react';

import ToastItem from '~/components/commons/Toasts/ToastItem';
import { getToasts, subscribeToasts } from '~/usecase/util/toasts';

import styles from './styles.module.scss';

const Toasts = () => {
  const toasts = React.useSyncExternalStore(subscribeToasts, getToasts);
  if (toasts.length === 0) return null;

  return (
    <div className={styles.stack} aria-live="polite">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} />
      ))}
    </div>
  );
};

export default Toasts;
