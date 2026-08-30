import { expect, test, mock } from 'bun:test';

const memory = new Map<string, string>();

globalThis.localStorage = {
  get length() {
    return memory.size;
  },
  key: (index: number) => [...memory.keys()][index] ?? null,
  getItem: (key: string) => memory.get(key) ?? null,
  setItem: (key: string, value: string) => void memory.set(key, value),
  removeItem: (key: string) => void memory.delete(key),
  clear: () => memory.clear()
} as Storage;

let stored: Record<string, string> | null = null;

mock.module('~/adapter/store/store.client', () => ({
  storeRead: async () => (stored ? { ...stored } : null),
  storeWrite: async (_name: string, value: Record<string, string>) => {
    stored = { ...value };
  }
}));

const { initPrefs, getPref, setPref, removePref } = await import('~/usecase/util/prefs');

test('imports legacy keys and drains localStorage', async () => {
  stored = null;
  localStorage.setItem('panorama:navWidth', '320');
  localStorage.setItem('agent:draft:abc', '{"text":"oi"}');
  localStorage.setItem('unrelated', 'keep');

  await initPrefs();

  expect(getPref('panorama:navWidth')).toBe('320');
  expect(getPref('agent:draft:abc')).toBe('{"text":"oi"}');
  expect(localStorage.getItem('panorama:navWidth')).toBeNull();
  expect(localStorage.getItem('unrelated')).toBe('keep');
});

test('stored value wins over a stale localStorage copy', async () => {
  stored = { 'panorama:navTab': 'git' };
  localStorage.setItem('panorama:navTab', 'files');
  localStorage.setItem('panorama:navOpen', '1');

  await initPrefs();

  expect(getPref('panorama:navTab')).toBe('git');
  expect(getPref('panorama:navOpen')).toBe('1');
  expect(localStorage.getItem('panorama:navTab')).toBeNull();
});

test('writes are debounced into a single persisted object', async () => {
  stored = {};
  await initPrefs();

  setPref('panorama:navWidth', '240');
  setPref('panorama:navWidth', '260');
  removePref('panorama:missing');
  expect(stored).toEqual({});

  await new Promise((resolve) => setTimeout(resolve, 400));
  expect(stored).toEqual({ 'panorama:navWidth': '260' });
});
