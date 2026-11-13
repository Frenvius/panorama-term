import React from 'react';

import { getPluginsState, PLUGINS_EVENT } from '~/usecase/service/plugins.service';

const subscribe = (onChange: () => void): (() => void) => {
  window.addEventListener(PLUGINS_EVENT, onChange);
  return () => window.removeEventListener(PLUGINS_EVENT, onChange);
};

export const usePlugins = () => React.useSyncExternalStore(subscribe, getPluginsState);
