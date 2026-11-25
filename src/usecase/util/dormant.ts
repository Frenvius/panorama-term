import type { Tile } from '~/domain/interfaces/workspace.interface';

import { showToast } from '~/usecase/util/toasts';
import { isHeadlessTile } from '~/usecase/util/agents';
import { getAgentStatuses } from '~/usecase/util/agentStatus';
import { isTerminalBusy } from '~/adapter/pty/sidecar.client';

export const canSleep = (tile: Tile): boolean =>
  tile.type === 'term' && !tile.runCwd && !tile.runId && !isHeadlessTile(tile) && !tile.ptySessionId;

export const hasSleepableTerminal = (tiles: Tile[]): boolean => tiles.some((t) => canSleep(t) && !t.dormant);

const isWorking = (id: string): Promise<boolean> => {
  const agent = getAgentStatuses().get(id);
  return agent ? Promise.resolve(agent === 'busy') : isTerminalBusy(id);
};

export const sleepIdleTerminals = async (tiles: Tile[], sleep: (id: string) => void): Promise<void> => {
  const candidates = tiles.filter((t) => canSleep(t) && !t.dormant);
  const working = await Promise.all(candidates.map((t) => isWorking(t.id)));
  const idle = candidates.filter((_, i) => !working[i]);
  idle.forEach((t) => sleep(t.id));
  const skipped = candidates.length - idle.length;
  const detail = skipped ? `${skipped} busy left running` : undefined;
  showToast({ kind: 'info', message: `Stopped ${idle.length} terminals`, detail });
};
