import type { PluginManifest, PluginSource, PluginsState } from '~/domain/interfaces/plugin.interface';

import { onPluginsChanged, scanPlugins } from '~/adapter/plugins/plugins.client';
import {
  readPlugins,
  toggleDisabled,
  enabledManifests,
  getDisabledPlugins,
  setDisabledPlugins
} from '~/usecase/util/plugins';

export const PLUGINS_EVENT = 'panorama-plugins';

let sources: PluginSource[] = [];
let state: PluginsState = { root: null, plugins: [], error: null };

const publish = (next: Partial<PluginsState>): void => {
  state = { ...state, ...next, plugins: readPlugins(sources, getDisabledPlugins()) };
  window.dispatchEvent(new Event(PLUGINS_EVENT));
};

export const getPluginsState = (): PluginsState => state;

export const enabledPlugins = (): PluginManifest[] => enabledManifests(state.plugins);

export const refreshPlugins = async (): Promise<void> => {
  try {
    const scan = await scanPlugins();
    sources = scan.plugins;
    publish({ root: scan.root, error: null });
  } catch (e) {
    publish({ error: String(e) });
  }
};

export const togglePlugin = (id: string): void => {
  setDisabledPlugins(toggleDisabled(getDisabledPlugins(), id));
  publish({});
};

let watching = false;

export const initPlugins = (): Promise<void> => {
  if (!watching) {
    watching = true;
    onPluginsChanged(() => void refreshPlugins());
  }
  return refreshPlugins();
};
