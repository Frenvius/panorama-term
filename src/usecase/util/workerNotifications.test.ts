import { expect, test } from 'bun:test';

import { muteWorkerNotifications } from '~/usecase/util/agentNotify';

import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';

test('worker notifications are muted for every kind', () => {
  const tile: Tile = { id: 'worker', type: 'term', x: 0, y: 0, width: 1, height: 1, zIndex: 0 };
  const kinds: NotifyKind[] = ['finished', 'attention', 'permission', 'idle', 'generic'];
  for (const _kind of kinds) {
    expect(muteWorkerNotifications({ ...tile, orchWorker: true })).toBe(true);
    expect(muteWorkerNotifications({ ...tile, runId: 'legacy' })).toBe(true);
    expect(muteWorkerNotifications(tile)).toBe(false);
    expect(muteWorkerNotifications({ ...tile, type: 'orch', runId: 'board' })).toBe(false);
  }
});
