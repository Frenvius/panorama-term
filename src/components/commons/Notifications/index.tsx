import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { X, Bell } from 'lucide-react';

import ClaudeLogo from '~/components/commons/ClaudeLogo';

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

const SOFT_KINDS: NotifyKind[] = ['finished', 'idle', 'generic'];

const playSound = (kind: NotifyKind): void => {
  const soft = SOFT_KINDS.includes(kind);
  const audio = new Audio(soft ? finishedSound : attentionSound);
  audio.volume = soft ? 0.2 : 0.7;
  audio.play().catch(() => {});
};

const KIND_TEXT: Record<NotifyKind, string> = {
  finished: 'Claude finished',
  attention: 'Claude needs your attention',
  permission: 'Claude needs permission',
  idle: 'Claude is waiting for input',
  generic: ''
};

const kindIcon = (kind: NotifyKind): React.ReactNode => {
  if (kind === 'generic') return <Bell size={20} />;
  return <ClaudeLogo size={20} />;
};

const NotificationOverlay = () => {
  const [toasts, setToasts] = React.useState<NotifyPayload[]>([]);
  const [expanded, setExpanded] = React.useState(false);
  const [itemH, setItemH] = React.useState(0);
  const frontRef = React.useRef<HTMLDivElement>(null);
  const hoverTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  React.useEffect(() => () => clearTimeout(hoverTimer.current), []);

  React.useEffect(() => {
    const shown = listen<NotifyPayload>('notif:show', (e) => {
      setToasts((prev) => {
        const kept = prev.filter((t) => t.tileId !== e.payload.tileId);
        return [...kept, e.payload].slice(-MAX_TOASTS);
      });
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

  const expand = () => {
    clearTimeout(hoverTimer.current);
    setExpanded(true);
  };

  const collapse = () => {
    hoverTimer.current = setTimeout(() => setExpanded(false), 140);
  };

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
          <div className={SOFT_KINDS.includes(toast.kind) ? styles.iconOk : styles.iconAsk}>
            {kindIcon(toast.kind)}
          </div>
          <div className={styles.body}>
            <div className={styles.title}>{toast.title}</div>
            <div className={styles.text}>{toast.text || KIND_TEXT[toast.kind]}</div>
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
