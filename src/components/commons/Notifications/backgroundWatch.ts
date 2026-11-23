import React from 'react';

import { openPtyConnection } from '~/adapter/pty/pty.client';
import { setAgentStatus, toAgentStatus } from '~/usecase/util/agentStatus';
import { createAgentNotifier, muteWorkerNotifications } from '~/usecase/util/agentNotify';
import { clearAlert } from '~/usecase/util/alerts';
import { workspaceService } from '~/usecase/service/workspace.service';
import { backgroundTargets } from '~/usecase/util/backgroundTargets';
import { getWorkspaceVersion, subscribeWorkspaces } from '~/usecase/service/workspace.service';
import { clearNotify, notifyBackground } from '~/components/commons/Notifications/bridge';
import { isTerminalLive, getLiveVersion, subscribeLiveTerminals } from '~/usecase/util/liveTerminals';

import type { WatchTarget } from '~/usecase/util/backgroundTargets';

const SETTLE_MS = 1000;

const collectTargets = async (): Promise<WatchTarget[]> => {
  const { workspaces } = await workspaceService.list();
  const all = await Promise.all(workspaces.map(async ({ id }) => ({ id, tabs: await workspaceService.peekTabs(id) })));
  for (const { tabs } of all) for (const tab of tabs) for (const tile of tab.state.tiles) {
    if (muteWorkerNotifications(tile)) clearAlert(tile.id);
  }
  return backgroundTargets(all, () => false);
};

const openWatch = (target: WatchTarget): WebSocket => {
  const { tileId } = target;
  const notifier = createAgentNotifier({
    tileId,
    suppressed: () => false,
    notify: (kind, text, label) => notifyBackground({ tileId, wsId: target.wsId, tabId: target.tabId }, kind, text, label || target.title),
    clear: () => clearNotify(tileId)
  });

  let ready = false;
  let status: string | undefined;

  return openPtyConnection(
    { tileId: target.sessionKey, cols: 80, rows: 24, watch: true },
    {
      acceptGrid: () => false,
      onGrid: () => {},
      onExit: () => setAgentStatus(tileId, null),
      onCwd: () => {},
      onTitle: () => {},
      onProgress: () => {},
      onClipboard: () => {},
      onPromptRestore: () => {},
      onReady: () => {
        ready = true;
      },
      onClaude: (state) => {
        if (state.reset) status = undefined;
        const next = state.status;
        const known = next ?? state.lastStatus;
        if (known) setAgentStatus(tileId, toAgentStatus(known));
        if (!next || next === status) return;
        const prev = status;
        status = next;
        if (ready) notifier.onStatus(prev, next, state.agent);
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

export const useBackgroundNotify = (): void => {
  const [targets, setTargets] = React.useState<WatchTarget[]>([]);
  const watchers = React.useRef(new Map<string, { ws: WebSocket; target: WatchTarget }>());
  const liveVersion = React.useSyncExternalStore(subscribeLiveTerminals, getLiveVersion);
  const workspaceVersion = React.useSyncExternalStore(subscribeWorkspaces, getWorkspaceVersion);

  React.useEffect(() => {
    let stale = false;
    void collectTargets().then((next) => {
      if (!stale) setTargets(next);
    });
    return () => {
      stale = true;
    };
  }, [workspaceVersion]);

  React.useEffect(() => {
    const wanted = new Map(targets.filter((t) => !isTerminalLive(t.tileId)).map((t) => [t.tileId, t]));
    for (const [tileId, target] of wanted) {
      const existing = watchers.current.get(tileId);
      if (existing && existing.target.sessionKey !== target.sessionKey) {
        existing.ws.close();
        watchers.current.delete(tileId);
      } else if (existing) Object.assign(existing.target, target);
      if (!watchers.current.has(tileId)) watchers.current.set(tileId, { target, ws: openWatch(target) });
    }
    const timer = setTimeout(() => {
      for (const [tileId, { ws }] of watchers.current) {
        if (wanted.has(tileId)) continue;
        watchers.current.delete(tileId);
        ws.close();
      }
    }, SETTLE_MS);
    return () => clearTimeout(timer);
  }, [targets, liveVersion]);

  React.useEffect(() => {
    const open = watchers.current;
    return () => {
      open.forEach(({ ws }) => ws.close());
      open.clear();
    };
  }, []);
};
