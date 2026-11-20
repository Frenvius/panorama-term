import React from 'react';

import { watchTileRequests } from '~/adapter/ui/ui.client';

export const useRemoteTiles = (openTile: (id: string, cwd: string, group?: { id: string; title: string }) => void) => {
  const openRef = React.useRef(openTile);
  openRef.current = openTile;

  React.useEffect(
    () => watchTileRequests((request) => openRef.current(request.tileId, request.cwd, request.group)),
    []
  );
};
