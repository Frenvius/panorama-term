import React from 'react';
import { X, Info, Check, Loader2, CircleAlert } from 'lucide-react';

import type { Toast, ToastKind } from '~/usecase/util/toasts';
import { dismissToast } from '~/usecase/util/toasts';

import styles from './styles.module.scss';

const ICONS: Record<ToastKind, React.ReactNode> = {
  info: <Info size={14} strokeWidth={2} />,
  success: <Check size={14} strokeWidth={2.5} />,
  error: <CircleAlert size={14} strokeWidth={2} />,
  progress: <Loader2 size={14} strokeWidth={2} className={styles.spinning} />
};

interface ToastItemProps {
  toast: Toast;
}

const ToastItem = ({ toast }: ToastItemProps) => {
  const close = () => dismissToast(toast.id);

  const runAction = () => {
    dismissToast(toast.id);
    toast.action?.run();
  };

  return (
    <div className={styles.toast} data-kind={toast.kind} role={toast.kind === 'error' ? 'alert' : 'status'}>
      <span className={styles.icon}>{ICONS[toast.kind]}</span>
      <div className={styles.text}>
        <span className={styles.message}>{toast.message}</span>
        {toast.detail && <span className={styles.detail}>{toast.detail}</span>}
      </div>
      {toast.action && (
        <button type="button" className={styles.action} onClick={runAction}>
          {toast.action.label}
        </button>
      )}
      {toast.kind !== 'progress' && (
        <button type="button" className={styles.close} onClick={close} aria-label="Dismiss">
          <X size={12} strokeWidth={2} />
        </button>
      )}
    </div>
  );
};

export default ToastItem;
