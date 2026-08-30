import React from 'react';
import { emit, listen } from '@tauri-apps/api/event';

import { stripSpinner } from '~/usecase/util/title';

import type { Tile } from '~/domain/interfaces/canvas.interface';
import type { NotifyKind, NotifyPayload, NotifyTarget } from '~/domain/interfaces/notify.interface';

const NOTIFY_EVENT = 'panorama:notify';
const NOTIFY_CLEAR_EVENT = 'panorama:notify-clear';

interface NotifyDetail {
  tileId: string;
  kind: NotifyKind;
  text?: string;
  title?: string;
  target?: NotifyTarget;
}

interface BridgeArgs {
  tiles: Tile[];
  wsId: string | null;
  tabId: string | null;
  activeTile: string | null;
  onOpen: (target: NotifyTarget) => void;
  onAlert: (tileId: string, kind: NotifyKind, tabId: string | null) => void;
  onClear: (tileId: string) => void;
}

let seq = 0;

export const notifyClaude = (tileId: string, kind: NotifyKind, text?: string, title?: string): void => {
  window.dispatchEvent(new CustomEvent<NotifyDetail>(NOTIFY_EVENT, { detail: { tileId, kind, text, title } }));
};

export const notifyBackground = (target: NotifyTarget, kind: NotifyKind, text?: string, title?: string): void => {
  const detail: NotifyDetail = { tileId: target.tileId, kind, text, title, target };
  window.dispatchEvent(new CustomEvent<NotifyDetail>(NOTIFY_EVENT, { detail }));
};

export const clearNotify = (tileId: string): void => {
  window.dispatchEvent(new CustomEvent<{ tileId: string }>(NOTIFY_CLEAR_EVENT, { detail: { tileId } }));
};

const tileTitle = (tile: Tile | undefined): string => {
  if (tile?.userTitle) return tile.userTitle;
  const osc = tile?.oscTitle && stripSpinner(tile.oscTitle);
  if (osc) return osc;
  const cwd = tile?.cwd;
  if (cwd) return cwd.split(/[\\/]/).filter(Boolean).pop() ?? 'Terminal';
  return 'Terminal';
};

export const useNotifyBridge = ({ tiles, wsId, tabId, activeTile, onOpen, onAlert, onClear }: BridgeArgs): void => {
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;

  const scopeRef = React.useRef({ wsId, tabId });
  scopeRef.current = { wsId, tabId };

  const onOpenRef = React.useRef(onOpen);
  onOpenRef.current = onOpen;

  const onAlertRef = React.useRef(onAlert);
  onAlertRef.current = onAlert;

  const onClearRef = React.useRef(onClear);
  onClearRef.current = onClear;

  React.useEffect(() => {
    const onNotify = (e: Event) => {
      const detail = (e as CustomEvent<NotifyDetail>).detail;
      const tile = tilesRef.current.find((t) => t.id === detail.tileId);
      const target = detail.target ?? { ...scopeRef.current, tileId: detail.tileId };
      const payload: NotifyPayload = {
        id: ++seq,
        tileId: detail.tileId,
        wsId: target.wsId,
        tabId: target.tabId,
        kind: detail.kind,
        title: detail.title || tileTitle(tile),
        text: detail.text
      };
      void emit('notif:show', payload);
      onAlertRef.current(detail.tileId, detail.kind, target.tabId);
    };
    const onNotifyClear = (e: Event) => {
      const { tileId } = (e as CustomEvent<{ tileId: string }>).detail;
      void emit('notif:dismiss', { tileId });
      onClearRef.current(tileId);
    };
    window.addEventListener(NOTIFY_EVENT, onNotify);
    window.addEventListener(NOTIFY_CLEAR_EVENT, onNotifyClear);
    return () => {
      window.removeEventListener(NOTIFY_EVENT, onNotify);
      window.removeEventListener(NOTIFY_CLEAR_EVENT, onNotifyClear);
    };
  }, []);

  React.useEffect(() => {
    const unlisten = listen<NotifyTarget>('notif:open', (e) => onOpenRef.current(e.payload));
    return () => {
      void unlisten.then((off) => off());
    };
  }, []);

  React.useEffect(() => {
    if (!activeTile) return;
    void emit('notif:dismiss', { tileId: activeTile });
  }, [activeTile]);
};
