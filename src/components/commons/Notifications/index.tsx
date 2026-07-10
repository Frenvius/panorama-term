import React from 'react';
import { X, CircleCheck, CircleAlert } from 'lucide-react';

import type { Tile } from '~/domain/interfaces/canvas.interface';

import styles from './styles.module.scss';
import finishedSound from './notif-finished.wav';
import attentionSound from './notif-attention.wav';

export type NotifyKind = 'finished' | 'attention';

const NOTIFY_EVENT = 'panorama:notify';
const MAX_TOASTS = 4;

interface NotifyDetail {
  tileId: string;
  kind: NotifyKind;
}

interface Toast {
  id: number;
  tileId: string;
  kind: NotifyKind;
  title: string;
}

interface NotificationsProps {
  tiles: Tile[];
  activeTile: string | null;
  onOpen: (tileId: string) => void;
}

let seq = 0;

export const notifyClaude = (tileId: string, kind: NotifyKind): void => {
  window.dispatchEvent(new CustomEvent<NotifyDetail>(NOTIFY_EVENT, { detail: { tileId, kind } }));
};

const tileTitle = (tile: Tile | undefined): string => {
  if (tile?.userTitle) return tile.userTitle;
  const cwd = tile?.cwd;
  if (cwd) return cwd.split(/[\\/]/).filter(Boolean).pop() ?? 'Terminal';
  return 'Terminal';
};

const playSound = (kind: NotifyKind): void => {
  const audio = new Audio(kind === 'finished' ? finishedSound : attentionSound);
  audio.volume = kind === 'finished' ? 0.2 : 0.7;
  audio.play().catch(() => {});
};

const Notifications = ({ tiles, activeTile, onOpen }: NotificationsProps) => {
  const [toasts, setToasts] = React.useState<Toast[]>([]);
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;

  const dismiss = React.useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  React.useEffect(() => {
    const onNotify = (e: Event) => {
      const detail = (e as CustomEvent<NotifyDetail>).detail;
      const tile = tilesRef.current.find((t) => t.id === detail.tileId);
      const toast: Toast = {
        id: ++seq,
        tileId: detail.tileId,
        kind: detail.kind,
        title: tileTitle(tile)
      };
      setToasts((prev) => [...prev, toast].slice(-MAX_TOASTS));
      playSound(detail.kind);
    };
    window.addEventListener(NOTIFY_EVENT, onNotify);
    return () => window.removeEventListener(NOTIFY_EVENT, onNotify);
  }, []);

  React.useEffect(() => {
    if (!activeTile) return;
    setToasts((prev) => prev.filter((t) => t.tileId !== activeTile));
  }, [activeTile]);

  const open = (toast: Toast) => {
    onOpen(toast.tileId);
    dismiss(toast.id);
  };

  const close = (e: React.MouseEvent, id: number) => {
    e.stopPropagation();
    dismiss(id);
  };

  if (toasts.length === 0) return null;

  return (
    <div className={styles.stack}>
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

export default Notifications;
