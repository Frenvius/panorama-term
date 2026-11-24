import React from 'react';

import { watchTileRequests, watchWakeRequests } from '~/adapter/ui/ui.client';

export const useRemoteTiles = (openTile: (id: string, cwd: string, group?: { id: string; title: string }, orchWorker?: boolean) => void) => {
  const openRef = React.useRef(openTile);
  openRef.current = openTile;

  React.useEffect(
    () => watchTileRequests((request) => openRef.current(request.tileId, request.cwd, request.group, request.orchWorker)),
    []
  );
};

export const useRemoteWake = (wakeTile: (id: string) => void) => {
  const wakeRef = React.useRef(wakeTile);
  wakeRef.current = wakeTile;

  React.useEffect(() => watchWakeRequests((tileId) => wakeRef.current(tileId)), []);
};
