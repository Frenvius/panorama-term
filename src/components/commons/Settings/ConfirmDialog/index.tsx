import React from 'react';

import Dialog from '~/components/commons/Dialog';
import Button from '~/components/commons/Settings/Button';

import styles from './styles.module.scss';

interface ConfirmDialogProps {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

const ConfirmDialog = ({ title, message, confirmLabel = 'Confirm', danger, onConfirm, onCancel }: ConfirmDialogProps) => {
  React.useEffect(() => {
    const marker = document.createElement('span');
    marker.dataset.settingsLayer = 'true';
    document.body.appendChild(marker);
    return () => marker.remove();
  }, []);

  const footer = (
    <>
      <Button variant="ghost" onClick={onCancel}>Cancel</Button>
      <Button variant={danger ? 'danger' : 'primary'} onClick={onConfirm}>{confirmLabel}</Button>
    </>
  );

  return (
    <Dialog title={title} onClose={onCancel} footer={footer}>
      <p className={styles.message}>{message}</p>
    </Dialog>
  );
};

export default ConfirmDialog;
