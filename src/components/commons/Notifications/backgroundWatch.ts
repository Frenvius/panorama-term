import React from 'react';

import { openPtyConnection } from '~/adapter/pty/pty.client';
import { createAgentNotifier } from '~/usecase/util/agentNotify';
import { workspaceService } from '~/usecase/service/workspace.service';
import { clearNotify, notifyBackground } from '~/components/commons/Notifications/bridge';
import { isTerminalLive, getLiveVersion, subscribeLiveTerminals } from '~/usecase/util/liveTerminals';

import type { Tile } from '~/domain/interfaces/workspace.interface';

const SETTLE_MS = 1000;

interface WatchTarget {
  tileId: string;
  tabId: string;
  title: string;
  sessionKey: string;
}

const tileLabel = (tile: Tile, tabName: string): string =>
  tile.userTitle || tile.cwd?.split(/[\\/]/).filter(Boolean).pop() || tabName;

const collectTargets = async (wsId: string): Promise<WatchTarget[]> => {
  const { tabs } = await workspaceService.getTabs(wsId);
  const out: WatchTarget[] = [];
  for (const tab of tabs) {
    const state = await workspaceService.loadTabState(wsId, tab.id);
    for (const tile of state?.tiles ?? []) {
      if (tile.type !== 'term' || tile.runCwd) continue;
      out.push({
        tileId: tile.id,
        tabId: tab.id,
        title: tileLabel(tile, tab.name),
        sessionKey: tile.ptySessionId ?? tile.id
      });
    }
  }
  return out;
};

const openWatch = (target: WatchTarget, wsId: string): WebSocket => {
  const { tileId, tabId, title } = target;
  const notifier = createAgentNotifier({
    suppressed: () => false,
    notify: (kind, text, label) => notifyBackground({ tileId, wsId, tabId }, kind, text, label || title),
    clear: () => clearNotify(tileId)
  });

  let ready = false;
  let status: string | undefined;

  return openPtyConnection(
    { tileId: target.sessionKey, cols: 80, rows: 24, watch: true },
    {
      acceptGrid: () => false,
      onGrid: () => {},
      onExit: () => {},
      onCwd: () => {},
      onTitle: () => {},
      onProgress: () => {},
      onClipboard: () => {},
      onReady: () => {
        ready = true;
      },
      onClaude: (state) => {
        if (state.reset) status = undefined;
        const next = state.status;
        if (!ready || !next || next === status) return;
        const prev = status;
        status = next;
        notifier.onStatus(prev, next);
      },
      onNotify: (label, body) => {
        if (ready) notifier.onNotify(label, body);
      },
      onAgentEvent: (evt) => {
        if (ready) notifier.onAgentEvent(evt);
      }
    }
  );
};

export const useBackgroundNotify = (wsId: string | null, tabKey: string): void => {
  const [targets, setTargets] = React.useState<WatchTarget[]>([]);
  const watchers = React.useRef(new Map<string, WebSocket>());
  const liveVersion = React.useSyncExternalStore(subscribeLiveTerminals, getLiveVersion);

  React.useEffect(() => {
    if (!wsId) {
      setTargets([]);
      return;
    }
    let stale = false;
    void collectTargets(wsId).then((next) => {
      if (!stale) setTargets(next);
    });
    return () => {
      stale = true;
    };
  }, [wsId, tabKey]);

  React.useEffect(() => {
    if (!wsId) return;
    const timer = setTimeout(() => {
      const wanted = new Map(targets.filter((t) => !isTerminalLive(t.tileId)).map((t) => [t.tileId, t]));
      for (const [tileId, ws] of watchers.current) {
        if (wanted.has(tileId)) continue;
        watchers.current.delete(tileId);
        ws.close();
      }
      for (const [tileId, target] of wanted) {
        if (watchers.current.has(tileId)) continue;
        watchers.current.set(tileId, openWatch(target, wsId));
      }
    }, SETTLE_MS);
    return () => clearTimeout(timer);
  }, [wsId, targets, liveVersion]);

  React.useEffect(() => {
    const open = watchers.current;
    return () => {
      open.forEach((ws) => ws.close());
      open.clear();
    };
  }, []);
};
