import { muteWorkerNotifications } from '~/usecase/util/agentNotify';

import type { TabState } from '~/domain/interfaces/workspace.interface';

export interface WatchTarget {
  tileId: string;
  wsId: string;
  tabId: string;
  title: string;
  sessionKey: string;
}

export const backgroundTargets = (workspaces: { id: string; tabs: TabState[] }[], isLive: (id: string) => boolean): WatchTarget[] =>
  workspaces.flatMap(({ id, tabs }) => tabs.flatMap((tab) => tab.state.tiles
    .filter((tile) => tile.type === 'term' && !tile.runCwd && !tile.dormant && !muteWorkerNotifications(tile) && !isLive(tile.id))
    .map((tile) => ({
      tileId: tile.id,
      wsId: id,
      tabId: tab.id,
      title: tile.userTitle || tile.cwd?.split(/[\\/]/).filter(Boolean).pop() || tab.name,
      sessionKey: tile.ptySessionId ?? tile.id
    }))));
