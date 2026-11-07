import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { PluginScan } from '~/domain/interfaces/plugin.interface';

export const scanPlugins = (): Promise<PluginScan> => invoke<PluginScan>('plugins_scan');

export const onPluginsChanged = (handler: () => void): (() => void) => {
  const off = listen('plugins:changed', handler);
  return () => void off.then((un) => un());
};
