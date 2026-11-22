import type { AgyTile, PerTileAccountKind } from '~/domain/interfaces/account.interface';

import { showToast } from '~/usecase/util/toasts';
import { resumeCommand } from '~/usecase/util/agentHistory';
import { requestTileRestart } from '~/usecase/util/tileRestart';
import { activateAgyAccount, setTileAccount } from '~/adapter/accounts/accounts.client';
import { getTileActivity, subscribeTileActivity } from '~/usecase/util/tileActivity';

const BARE_COMMAND: Record<PerTileAccountKind, string> = { codex: 'codex', claude: 'claude' };

const restartCommand = (agent: string, sessionId: string | null | undefined, bare: string): string =>
  (sessionId && resumeCommand(agent, sessionId)) || bare;

export const isTileBusy = (tileId: string): boolean => getTileActivity().get(tileId)?.busySince !== undefined;

export const tileSessionId = (tileId: string): string | undefined => getTileActivity().get(tileId)?.sessionId;

interface SwitchTileOpts {
  id: string;
  tileId: string;
  agent: PerTileAccountKind;
}

export const switchTileAccount = async ({ id, tileId, agent }: SwitchTileOpts): Promise<void> => {
  await setTileAccount({ id, tileId, agent });
  requestTileRestart(tileId, restartCommand(agent, tileSessionId(tileId), BARE_COMMAND[agent]));
};

const agyCommand = (tile: AgyTile): string => restartCommand('antigravity', tile.conversationId, 'agy');

const restartWhenIdle = (tile: AgyTile): void => {
  const command = agyCommand(tile);
  const off = subscribeTileActivity(() => {
    if (isTileBusy(tile.tileId)) return;
    off();
    requestTileRestart(tile.tileId, command);
  });
};

export const switchAgyAccount = async (id: string, name: string): Promise<void> => {
  const { tiles } = await activateAgyAccount(id);
  for (const tile of tiles) {
    if (tile.status === null || tile.status === 'idle') requestTileRestart(tile.tileId, agyCommand(tile));
    else restartWhenIdle(tile);
  }
  const count = tiles.length;
  const suffix = count > 0 ? `. Restarting ${count} terminal${count === 1 ? '' : 's'}` : '';
  showToast({ kind: 'success', message: `Switched Antigravity to ${name}${suffix}` });
};
