import React from 'react';

import { termMetrics } from '~/usecase/util/fonts';
import { THEME_EVENT } from '~/usecase/util/theme';

const subscribe = (onChange: () => void): (() => void) => {
  window.addEventListener(THEME_EVENT, onChange);
  return () => window.removeEventListener(THEME_EVENT, onChange);
};

const cellWidth = () => termMetrics.cellW;

export const useTermCellWidth = (): number => React.useSyncExternalStore(subscribe, cellWidth);
