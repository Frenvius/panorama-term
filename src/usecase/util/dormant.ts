import type { Tile } from '~/domain/interfaces/workspace.interface';

export const canSleep = (tile: Tile): boolean =>
  tile.type === 'term' && !tile.runCwd && !tile.runId && !tile.orchWorker && !tile.ptySessionId;
