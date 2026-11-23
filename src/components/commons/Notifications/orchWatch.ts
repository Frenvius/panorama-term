import React from 'react';

import { watchOrchNotifications } from '~/adapter/ui/ui.client';
import { workspaceService } from '~/usecase/service/workspace.service';
import { orchNotificationText } from '~/usecase/util/orchNotify';
import { getNotifToasts } from '~/usecase/util/notifPlacement';
import { notifyBackground } from '~/components/commons/Notifications/bridge';

import type { Tile, WorkspaceMeta } from '~/domain/interfaces/workspace.interface';

interface WatchArgs {
  tiles: Tile[];
  workspaces: WorkspaceMeta[];
  wsId: string | null;
  tabId: string | null;
  suppressed: (tile: Tile) => boolean;
}

const recent = new Map<string, number>();

export const useOrchNotifications = (args: WatchArgs): void => {
  const current = React.useRef(args);
  current.current = args;
  React.useEffect(() => watchOrchNotifications((event) => {
    const now = Date.now();
    const key = JSON.stringify([event.runId, event.kind, event.taskId]);
    for (const [id, at] of recent) if (now - at > 5000) recent.delete(id);
    if (recent.has(key) || !getNotifToasts()) return;
    recent.set(key, now);
    void (async () => {
      const state = current.current;
      let tile = state.tiles.find((item) => item.type === 'orch' && item.runId === event.runId);
      let wsId = state.wsId;
      let tabId = state.tabId;
      if (tile && state.suppressed(tile)) return;
      if (!tile) {
        for (const ws of state.workspaces) {
          const { tabs } = await workspaceService.getTabs(ws.id);
          for (const tab of tabs) {
            if (ws.id === state.wsId && tab.id === state.tabId) continue;
            const saved = await workspaceService.loadTabState(ws.id, tab.id);
            tile = saved?.tiles.find((item) => item.type === 'orch' && item.runId === event.runId);
            if (tile) { wsId = ws.id; tabId = tab.id; break; }
          }
          if (tile) break;
        }
      }
      notifyBackground({ tileId: tile?.id ?? `orch:${event.runId}`, wsId, tabId, runId: event.runId }, 'attention', orchNotificationText(event), event.runTitle);
    })().catch(() => {});
  }), []);
};
