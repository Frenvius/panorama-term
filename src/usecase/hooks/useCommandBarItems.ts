import type { CommandBarItem } from '~/domain/interfaces/commandBar.interface';
import type { NotifyKind, NotifyTarget } from '~/domain/interfaces/notify.interface';
import type { Tile, Frame, WorkspaceMeta } from '~/domain/interfaces/workspace.interface';

import React from 'react';

import { tileLabel } from '~/usecase/util/title';
import { byRecency } from '~/usecase/util/tileRecency';
import { workspaceService } from '~/usecase/service/workspace.service';
import { SETTINGS_SECTIONS, requestSettings } from '~/usecase/util/settingsNav';
import { KEYBINDINGS, getBinding, formatCombo, type CommandId } from '~/usecase/util/keybindings';

const BAR_COMMANDS = new Set<CommandId>([
  'tile.new',
  'note.new',
  'tile.close',
  'tile.focus',
  'tile.reopen',
  'tile.fullscreen',
  'view.resetZoom',
  'view.navigator'
]);

export interface CommandBarActions {
  focusTile: (id: string) => void;
  focusFrame: (id: string) => void;
  runCommand: (id: CommandId) => void;
  openWorkspace: (wsId: string) => void;
  openTile: (target: NotifyTarget) => void;
  openTab: (wsId: string, tabId: string) => void;
}

export interface CommandBarSources {
  tiles: Tile[];
  frames: Frame[];
  alerts: Map<string, NotifyKind>;
  activeId: string | null;
  actions: CommandBarActions;
  activeTabId: string | null;
  workspaces: WorkspaceMeta[];
}

const tilePath = (tile: Tile): string => tile.filePath ?? tile.folderPath ?? tile.cwd ?? tile.url ?? '';

const tileItem = (tile: Tile, alerts: CommandBarSources['alerts'], run: () => void): CommandBarItem => ({
  run,
  id: `tile:${tile.id}`,
  kind: 'tile',
  label: tileLabel(tile),
  detail: tilePath(tile),
  alert: alerts.has(tile.id),
  tileType: tile.type
});

const commandItems = (runCommand: CommandBarActions['runCommand']): CommandBarItem[] =>
  KEYBINDINGS.filter((cmd) => BAR_COMMANDS.has(cmd.id)).map((cmd) => {
    const combo = getBinding(cmd.id);
    return {
      id: `command:${cmd.id}`,
      kind: 'command',
      label: cmd.label,
      hint: combo ? formatCombo(combo) : undefined,
      run: () => runCommand(cmd.id)
    };
  });

const settingItems = (): CommandBarItem[] =>
  SETTINGS_SECTIONS.map((s) => ({
    id: `setting:${s.id}`,
    kind: 'setting',
    label: s.label,
    detail: 'Settings',
    run: () => requestSettings(s.id)
  }));

const remoteItems = async (sources: CommandBarSources): Promise<CommandBarItem[]> => {
  const { workspaces, activeId, activeTabId, alerts, actions } = sources;
  const perWorkspace = await Promise.all(
    workspaces.map(async (ws) => ({ ws, tabs: await workspaceService.peekTabs(ws.id) }))
  );
  const items: CommandBarItem[] = [];
  for (const { ws, tabs } of perWorkspace) {
    const local = ws.id === activeId;
    for (const tab of tabs) {
      if (local && tab.id === activeTabId) continue;
      const scope = local ? tab.name : `${ws.name} / ${tab.name}`;
      items.push({
        id: `tab:${ws.id}:${tab.id}`,
        kind: 'tab',
        label: tab.name,
        scope: local ? undefined : ws.name,
        color: ws.color,
        run: () => actions.openTab(ws.id, tab.id)
      });
      for (const tile of tab.state.tiles) {
        const target = { tileId: tile.id, wsId: ws.id, tabId: tab.id };
        items.push({ ...tileItem(tile, alerts, () => actions.openTile(target)), scope });
      }
    }
  }
  return items;
};

export const useCommandBarItems = (sources: CommandBarSources): CommandBarItem[] => {
  const [remote, setRemote] = React.useState<CommandBarItem[]>([]);

  React.useEffect(() => {
    let alive = true;
    remoteItems(sources)
      .then((items) => alive && setRemote(items))
      .catch((err) => console.error('command bar: failed to read other tabs', err));
    return () => {
      alive = false;
    };
  }, []);

  const { tiles, frames, alerts, actions, workspaces, activeId } = sources;

  return React.useMemo(() => {
    const local = byRecency(tiles).map((tile) => tileItem(tile, alerts, () => actions.focusTile(tile.id)));
    const frameItems: CommandBarItem[] = frames.map((f) => ({
      id: `frame:${f.id}`,
      kind: 'frame',
      label: f.title || 'Untitled frame',
      color: f.color,
      run: () => actions.focusFrame(f.id)
    }));
    const workspaceItems: CommandBarItem[] = workspaces
      .filter((ws) => ws.id !== activeId)
      .map((ws) => ({
        id: `workspace:${ws.id}`,
        kind: 'workspace',
        label: ws.name,
        color: ws.color,
        run: () => actions.openWorkspace(ws.id)
      }));
    return [...local, ...remote, ...frameItems, ...workspaceItems, ...commandItems(actions.runCommand), ...settingItems()];
  }, [tiles, frames, alerts, actions, workspaces, activeId, remote]);
};
