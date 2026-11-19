import type { DockLayout, DockMode, DockSide, PanelId } from '~/domain/interfaces/dock.interface';

import React from 'react';

import { getDockSnapshot, subscribeDock, dockLayoutActions } from '~/usecase/store/dockLayout';

export const useDockLayout = (
  mode: DockMode
): {
  layout: DockLayout;
  move: (id: PanelId, side: DockSide) => void;
  hide: (id: PanelId) => void;
  show: (id: PanelId) => void;
  setActive: (side: DockSide, id: PanelId) => void;
  reset: () => void;
} => {
  const subscribe = React.useCallback((fn: () => void) => subscribeDock(mode, fn), [mode]);
  const getSnapshot = React.useCallback(() => getDockSnapshot(mode), [mode]);

  const layout = React.useSyncExternalStore(subscribe, getSnapshot);

  const move = React.useCallback((id: PanelId, side: DockSide) => dockLayoutActions.move(mode, id, side), [mode]);
  const hide = React.useCallback((id: PanelId) => dockLayoutActions.hide(mode, id), [mode]);
  const show = React.useCallback((id: PanelId) => dockLayoutActions.show(mode, id), [mode]);
  const setActive = React.useCallback((side: DockSide, id: PanelId) => dockLayoutActions.setActive(mode, side, id), [mode]);
  const reset = React.useCallback(() => dockLayoutActions.reset(mode), [mode]);

  return { layout, move, hide, show, setActive, reset };
};
