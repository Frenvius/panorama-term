import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { X, CircleCheck, MessageCircleQuestionMark } from 'lucide-react';

import type { NotifyKind, NotifyPayload } from '~/components/commons/Notifications/bridge';

import styles from './styles.module.scss';
import finishedSound from './notif-finished.wav';
import attentionSound from './notif-attention.wav';

const GAP = 10;
const PAD_TOP = 16;
const MAX_PEEK = 3;
const MAX_TOASTS = 6;
const PAD_BOTTOM = 32;
const PEEK_SCALE = 0.05;
const PEEK_OFFSET = 12;

const playSound = (kind: NotifyKind): void => {
  const audio = new Audio(kind === 'finished' ? finishedSound : attentionSound);
  audio.volume = kind === 'finished' ? 0.2 : 0.7;
  audio.play().catch(() => {});
};

const NotificationOverlay = () => {
  const [toasts, setToasts] = React.useState<NotifyPayload[]>([]);
  const [expanded, setExpanded] = React.useState(false);
  const [itemH, setItemH] = React.useState(0);
  const frontRef = React.useRef<HTMLDivElement>(null);

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

  React.useLayoutEffect(() => {
    const h = frontRef.current?.offsetHeight ?? 0;
    if (h) setItemH(h);
  }, [toasts]);

  React.useEffect(() => {
    const n = toasts.length;
    const peek = Math.min(n - 1, MAX_PEEK) * PEEK_OFFSET;
    const fanned = n * itemH + (n - 1) * GAP;
    const content = expanded ? fanned : itemH + peek;
    const height = n === 0 || itemH === 0 ? 0 : PAD_TOP + content + PAD_BOTTOM;
    void invoke('notif_layout', { height });
  }, [toasts, expanded, itemH]);

  const open = (toast: NotifyPayload) => {
    void emit('notif:open', { tileId: toast.tileId });
    void invoke('focus_main');
    setToasts((prev) => prev.filter((t) => t.id !== toast.id));
  };

  const close = (e: React.MouseEvent, id: number) => {
    e.stopPropagation();
    setToasts((prev) => prev.filter((t) => t.id !== id));
  };

  const expand = () => setExpanded(true);
  const collapse = () => setExpanded(false);

  const styleFor = (i: number): React.CSSProperties => {
    const n = toasts.length;
    if (expanded) {
      return { bottom: PAD_BOTTOM + (n - 1 - i) * (itemH + GAP), zIndex: i + 1 };
    }
    const depth = Math.min(n - 1 - i, MAX_PEEK + 1);
    return {
      bottom: PAD_BOTTOM,
      zIndex: n - depth,
      opacity: depth > MAX_PEEK ? 0 : 1,
      transform: `translateY(${-depth * PEEK_OFFSET}px) scale(${1 - depth * PEEK_SCALE})`,
      pointerEvents: depth === 0 ? 'auto' : 'none'
    };
  };

  return (
    <div className={styles.stack} onMouseEnter={expand} onMouseLeave={collapse}>
      {toasts.map((toast, i) => (
        <div
          key={toast.id}
          ref={i === toasts.length - 1 ? frontRef : null}
          className={styles.toast}
          style={styleFor(i)}
          onClick={() => open(toast)}
        >
          <div className={toast.kind === 'finished' ? styles.iconOk : styles.iconAsk}>
            {toast.kind === 'finished' ? (
              <CircleCheck size={20} />
            ) : (
              <MessageCircleQuestionMark size={20} />
            )}
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
