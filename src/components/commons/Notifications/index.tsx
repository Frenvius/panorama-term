import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { X, CircleCheck, CircleAlert } from 'lucide-react';

import type { NotifyKind, NotifyPayload } from '~/components/commons/Notifications/bridge';

import styles from './styles.module.scss';
import finishedSound from './notif-finished.wav';
import attentionSound from './notif-attention.wav';

const MAX_TOASTS = 4;

const playSound = (kind: NotifyKind): void => {
  const audio = new Audio(kind === 'finished' ? finishedSound : attentionSound);
  audio.volume = kind === 'finished' ? 0.2 : 0.7;
  audio.play().catch(() => {});
};

const NotificationOverlay = () => {
  const [toasts, setToasts] = React.useState<NotifyPayload[]>([]);
  const stackRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    const shown = listen<NotifyPayload>('notif:show', (e) => {
      setToasts((prev) => [...prev, e.payload].slice(-MAX_TOASTS));
      playSound(e.payload.kind);
    });
    const dismissed = listen<{ tileId: string }>('notif:dismiss', (e) => {
      setToasts((prev) => prev.filter((t) => t.tileId !== e.payload.tileId));
    });
    return () => {
      void shown.then((off) => off());
      void dismissed.then((off) => off());
    };
  }, []);

  React.useEffect(() => {
    const height = toasts.length === 0 ? 0 : (stackRef.current?.scrollHeight ?? 0);
    void invoke('notif_layout', { height });
  }, [toasts]);

  const open = (toast: NotifyPayload) => {
    void emit('notif:open', { tileId: toast.tileId });
    void invoke('focus_main');
    setToasts((prev) => prev.filter((t) => t.id !== toast.id));
  };

  const close = (e: React.MouseEvent, id: number) => {
    e.stopPropagation();
    setToasts((prev) => prev.filter((t) => t.id !== id));
  };

  return (
    <div ref={stackRef} className={styles.stack}>
      {toasts.map((toast) => (
        <div key={toast.id} className={styles.toast} onClick={() => open(toast)}>
          <div className={toast.kind === 'finished' ? styles.iconOk : styles.iconAlert}>
            {toast.kind === 'finished' ? <CircleCheck size={20} /> : <CircleAlert size={20} />}
          </div>
          <div className={styles.body}>
            <div className={styles.title}>{toast.title}</div>
            <div className={styles.text}>
              {toast.kind === 'finished' ? 'Claude finished' : 'Claude needs your attention'}
            </div>
          </div>
          <button className={styles.close} onClick={(e) => close(e, toast.id)}>
            <X size={16} />
          </button>
        </div>
      ))}
    </div>
  );
};

export default NotificationOverlay;
