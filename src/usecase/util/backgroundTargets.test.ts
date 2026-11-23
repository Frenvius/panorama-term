import { expect, test } from 'bun:test';

import { backgroundTargets } from '~/usecase/util/backgroundTargets';

import type { TabState, Tile } from '~/domain/interfaces/workspace.interface';

const tile = (id: string, extra: Partial<Tile> = {}): Tile => ({ id, type: 'term', x: 0, y: 0, width: 1, height: 1, zIndex: 0, ...extra });
const tab = (id: string, tiles: Tile[]): TabState => ({ id, name: id, state: { version: 1, tiles, viewport: { zoom: 1, centerX: 0, centerY: 0 } } });

test('watches unmounted terminals across tabs and workspaces, excluding workers', () => {
  const workspaces = [
    { id: 'one', tabs: [tab('first', [tile('mounted')]), tab('second', [tile('new'), tile('worker', { orchWorker: true })])] },
    { id: 'two', tabs: [tab('third', [tile('other')])] }
  ];
  expect(backgroundTargets(workspaces, (id) => id === 'mounted').map(({ tileId, wsId, tabId }) => [tileId, wsId, tabId]))
    .toEqual([['new', 'one', 'second'], ['other', 'two', 'third']]);
});
