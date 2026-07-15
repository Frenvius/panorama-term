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
  const [heights, setHeights] = React.useState<Record<number, number>>({});
  const nodes = React.useRef(new Map<number, HTMLDivElement>());
  const observer = React.useRef<ResizeObserver | null>(null);
  const hoverTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  if (!observer.current && typeof ResizeObserver !== 'undefined') {
    observer.current = new ResizeObserver(() => {
      setHeights((prev) => {
        const next: Record<number, number> = {};
        let changed = false;
        nodes.current.forEach((el, id) => {
          next[id] = el.offsetHeight;
          if (prev[id] !== next[id]) changed = true;
        });
        return changed ? next : prev;
      });
    });
  }

  React.useEffect(() => () => clearTimeout(hoverTimer.current), []);
  React.useEffect(() => () => observer.current?.disconnect(), []);

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

  const bottoms = React.useMemo(() => {
    const out: number[] = [];
    let acc = PAD_BOTTOM;
    for (let i = toasts.length - 1; i >= 0; i -= 1) {
      out[i] = acc;
      acc += (heights[toasts[i].id] ?? 0) + GAP;
    }
    return out;
  }, [toasts, heights]);

  React.useEffect(() => {
    const n = toasts.length;
    if (n === 0) {
      void invoke('notif_layout', { height: 0 });
      return;
    }
    if (toasts.some((t) => !heights[t.id])) {
      void invoke('notif_layout', { height: PAD_TOP + PAD_BOTTOM + 72 });
      return;
    }

    let top = 0;
    if (expanded) {
      top = bottoms[0] + heights[toasts[0].id];
    } else {
      toasts.forEach((t, i) => {
        const depth = n - 1 - i;
        if (depth > MAX_PEEK) return;
        const visible = heights[t.id] * (1 - depth * PEEK_SCALE);
        top = Math.max(top, PAD_BOTTOM + depth * PEEK_OFFSET + visible);
      });
    }
    void invoke('notif_layout', { height: PAD_TOP + top });
  }, [toasts, expanded, heights, bottoms]);

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

  const setNode = (id: number) => (el: HTMLDivElement | null) => {
    const prev = nodes.current.get(id);
    if (prev) observer.current?.unobserve(prev);
    if (el) {
      nodes.current.set(id, el);
      observer.current?.observe(el);
    } else {
      nodes.current.delete(id);
    }
  };

  const styleFor = (i: number): React.CSSProperties => {
    const n = toasts.length;
    if (expanded) {
      return { bottom: bottoms[i], zIndex: i + 1 };
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
    <div
      className={styles.stack}
      onMouseEnter={expand}
      onMouseLeave={collapse}
      onContextMenu={(e) => e.preventDefault()}
    >
      {toasts.map((toast, i) => (
        <div
          key={toast.id}
          ref={setNode(toast.id)}
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
