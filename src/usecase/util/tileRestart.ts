import React from 'react';

type RestartHandler = (command?: string) => void;

const handlers = new Map<string, RestartHandler>();
const deferred = new Map<string, string | undefined>();

export const requestTileRestart = (tileId: string, command?: string): boolean => {
  const handler = handlers.get(tileId);
  if (!handler) return false;
  handler(command);
  return true;
};

export const requestTileRestartOnMount = (tileId: string, command?: string): void => {
  if (!requestTileRestart(tileId, command)) deferred.set(tileId, command);
};

export const registerTileRestart = (tileId: string, handler: RestartHandler): (() => void) => {
  handlers.set(tileId, handler);
  if (deferred.has(tileId)) {
    const command = deferred.get(tileId);
    deferred.delete(tileId);
    handler(command);
  }
  return () => {
    if (handlers.get(tileId) === handler) handlers.delete(tileId);
  };
};

export const useTileRestart = (tileId: string, handler: RestartHandler): void => {
  const ref = React.useRef(handler);
  ref.current = handler;
  React.useEffect(() => registerTileRestart(tileId, (command) => ref.current(command)), [tileId]);
};
