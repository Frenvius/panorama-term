import React from 'react';
import { X } from 'lucide-react';

import styles from './styles.module.scss';

interface DialogProps {
  title: string;
  onClose: () => void;
  children: React.ReactNode;
  footer?: React.ReactNode;
  onSubmit?: () => void;
}

const Dialog = ({ title, onClose, children, footer, onSubmit }: DialogProps) => {
  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      onClose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  const onBackdrop = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  };

  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit?.();
  };

  return (
    <div className={styles.backdrop} onMouseDown={onBackdrop}>
      <form className={styles.dialog} onSubmit={submit}>
        <header className={styles.header}>
          <span className={styles.title}>{title}</span>
          <button type="button" className={styles.close} onClick={onClose} aria-label="Close">
            <X size={14} strokeWidth={2} />
          </button>
        </header>
        <div className={styles.body}>{children}</div>
        {footer && <div className={styles.footer}>{footer}</div>}
      </form>
    </div>
  );
};

export default Dialog;
