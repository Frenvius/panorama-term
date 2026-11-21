import type { TileHostValue, TileHostProviderProps } from './types';

import React from 'react';

export type { TileStatus, TileLink, TileHostValue, TileActions, BoardActions } from './types';

const TileHostContext = React.createContext<TileHostValue | null>(null);

export const TileHostProvider = ({ value, children }: TileHostProviderProps) => (
  <TileHostContext.Provider value={value}>{children}</TileHostContext.Provider>
);

export const useTileHost = (): TileHostValue => {
  const context = React.useContext(TileHostContext);
  if (!context) throw new Error('useTileHost must be used inside TileHostProvider');
  return context;
};
