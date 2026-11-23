import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type TabScope = 'project' | 'global';

const EVENT = 'panorama-tab-scope';

export const getTabScope = (): TabScope => getSetting<string>('tabScope', 'project') === 'global' ? 'global' : 'project';

export const setTabScope = (scope: TabScope): void => {
  void setSetting('tabScope', scope);
  window.dispatchEvent(new Event(EVENT));
};

export const subscribeTabScope = (listener: () => void): (() => void) => {
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
};

export const visibleTabs = <T extends { workspaceId: string }>(tabs: T[], activeProject: string | null, scope: TabScope): T[] =>
  scope === 'global' ? tabs : tabs.filter((tab) => tab.workspaceId === activeProject);

export const showWorkspacePrefix = (tabs: { workspaceId: string }[]): boolean =>
  new Set(tabs.map((tab) => tab.workspaceId)).size > 1;
