import type { TileOpenRequest } from '~/domain/interfaces/mobile.interface';

import { SIDECAR_WS } from '~/adapter/pty/sidecar.client';

const RECONNECT_MS = 2000;

export const watchTileRequests = (onOpen: (request: TileOpenRequest) => void): (() => void) => {
  let ws: WebSocket | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let closed = false;

  const connect = () => {
    if (closed) return;
    ws = new WebSocket(`${SIDECAR_WS}/ui`);
    ws.onmessage = (e) => {
      if (typeof e.data !== 'string') return;
      const msg = JSON.parse(e.data) as TileOpenRequest;
      if (msg.t === 'tile.open') onOpen(msg);
    };
    ws.onclose = () => {
      if (!closed) timer = setTimeout(connect, RECONNECT_MS);
    };
  };

  connect();
  return () => {
    closed = true;
    clearTimeout(timer);
    ws?.close();
  };
};
