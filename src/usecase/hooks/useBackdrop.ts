import React from 'react';

import { getBackdrop, THEME_EVENT, type Backdrop } from '~/usecase/util/theme';

const subscribe = (onChange: () => void): (() => void) => {
  window.addEventListener(THEME_EVENT, onChange);
  return () => window.removeEventListener(THEME_EVENT, onChange);
};

export const useBackdrop = (): Backdrop | null => React.useSyncExternalStore(subscribe, getBackdrop);
