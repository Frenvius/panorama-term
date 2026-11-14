import { expect, test } from 'bun:test';
import { createDeferredReload, shouldReload, shouldPersist, listenPointerInteraction } from './storeSync';

test('only changes from another screen affecting workspaces trigger reload', () => {
  expect(shouldReload({ name: 'workspaces/a.json', revision: 1, origin: 'other' }, 'self')).toBe(true);
  expect(shouldReload({ name: 'workspaces.json', revision: 2, origin: null }, 'self')).toBe(true);
  expect(shouldReload({ name: 'workspaces/a.json', revision: 3, origin: 'self' }, 'self')).toBe(false);
  expect(shouldReload({ name: 'config.json', revision: 4, origin: 'other' }, 'self')).toBe(false);
});

test('coalesces changes during interaction and applies them on release', () => {
  const calls: string[][] = [];
  const sync = createDeferredReload((names) => calls.push(names));
  sync.start();
  sync.change('workspaces/a.json');
  sync.change('workspaces/a.json');
  sync.change('workspaces.json');
  sync.change('*');
  expect(calls).toEqual([]);
  sync.end();
  expect(calls).toEqual([['workspaces/a.json', 'workspaces.json', '*']]);
  sync.end();
  expect(calls).toHaveLength(1);
});

test('applying a received snapshot does not persist it, but later local edits do', () => {
  const received = { tiles: [{ id: 'note', x: 10 }], frames: [] };
  const applied = { tiles: received.tiles, frames: received.frames };
  expect(shouldPersist(applied, received, true)).toBe(false);
  expect(shouldPersist(applied, received, false)).toBe(true);
  expect(shouldPersist(applied, { ...received, tiles: [{ id: 'note', x: 20 }] })).toBe(true);
  expect(shouldPersist(applied, { ...received, frames: [{}] })).toBe(true);
});

test('pointer listeners end interactions and are removed together', () => {
  const target = new EventTarget();
  const calls: string[] = [];
  const stop = listenPointerInteraction(
    target as Window,
    () => calls.push('start'),
    () => calls.push('end')
  );
  target.dispatchEvent(new Event('pointerdown'));
  target.dispatchEvent(new Event('pointercancel'));
  target.dispatchEvent(new Event('blur'));
  stop();
  target.dispatchEvent(new Event('pointerdown'));
  expect(calls).toEqual(['start', 'end', 'end']);
});
