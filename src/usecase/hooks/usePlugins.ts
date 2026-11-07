import React from 'react';

import type { PluginScan } from '~/domain/interfaces/plugin.interface';

import { onPluginsChanged, scanPlugins } from '~/adapter/plugins/plugins.client';
import { getDisabledPlugins, readPlugins, setDisabledPlugins, toggleDisabled } from '~/usecase/util/plugins';

export const usePlugins = () => {
  const [scan, setScan] = React.useState<PluginScan | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [disabled, setDisabled] = React.useState(getDisabledPlugins);

  React.useEffect(() => {
    const refresh = () =>
      scanPlugins()
        .then((next) => {
          setScan(next);
          setError(null);
        })
        .catch((e) => setError(String(e)));
    void refresh();
    return onPluginsChanged(() => void refresh());
  }, []);

  const plugins = React.useMemo(() => (scan ? readPlugins(scan.plugins, disabled) : []), [scan, disabled]);

  const toggle = (id: string) => {
    const next = toggleDisabled(disabled, id);
    setDisabled(next);
    setDisabledPlugins(next);
  };

  return { root: scan?.root ?? null, plugins, error, toggle };
};
