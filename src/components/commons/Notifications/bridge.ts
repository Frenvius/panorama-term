import React from 'react';
import { emit, listen } from '@tauri-apps/api/event';

import type { Tile } from '~/domain/interfaces/canvas.interface';

export type NotifyKind = 'finished' | 'attention';

export interface NotifyPayload {
  id: number;
  tileId: string;
  kind: NotifyKind;
  title: string;
}

const NOTIFY_EVENT = 'panorama:notify';

interface NotifyDetail {
  tileId: string;
  kind: NotifyKind;
}

interface BridgeArgs {
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

export const useNotifyBridge = ({ tiles, activeTile, onOpen }: BridgeArgs): void => {
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;

  const onOpenRef = React.useRef(onOpen);
  onOpenRef.current = onOpen;

  React.useEffect(() => {
    const onNotify = (e: Event) => {
      const detail = (e as CustomEvent<NotifyDetail>).detail;
      const tile = tilesRef.current.find((t) => t.id === detail.tileId);
      const payload: NotifyPayload = {
        id: ++seq,
        tileId: detail.tileId,
        kind: detail.kind,
        title: tileTitle(tile)
      };
      void emit('notif:show', payload);
    };
    window.addEventListener(NOTIFY_EVENT, onNotify);
    return () => window.removeEventListener(NOTIFY_EVENT, onNotify);
  }, []);

  React.useEffect(() => {
    const unlisten = listen<{ tileId: string }>('notif:open', (e) => onOpenRef.current(e.payload.tileId));
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  React.useEffect(() => {
    if (!activeTile) return;
    void emit('notif:dismiss', { tileId: activeTile });
  }, [activeTile]);
};
