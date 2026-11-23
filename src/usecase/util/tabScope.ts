import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type TabScope = 'project' | 'global';

const EVENT = 'panorama-tab-scope';

export const getTabScope = (): TabScope => (getSetting<string>('tabScope', 'project') === 'global' ? 'global' : 'project');

export const setTabScope = (scope: TabScope): void => {
  void setSetting('tabScope', scope);
  window.dispatchEvent(new Event(EVENT));
};

export const subscribeTabScope = (listener: () => void): (() => void) => {
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
};
