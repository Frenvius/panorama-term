import { expect, test, mock } from 'bun:test';

const settings = new Map<string, unknown>();

mock.module('~/adapter/settings/settings.client', () => ({
  getSetting: <T>(key: string, fallback: T): T => (settings.has(key) ? (settings.get(key) as T) : fallback),
  setSetting: async (key: string, value: unknown) => void settings.set(key, value)
}));

const { getNotifToasts, setNotifToasts, getNotifHideOverFullscreen } = await import('~/usecase/util/notifPlacement');

test('notification toasts are on by default', () => {
  settings.clear();
  expect(getNotifToasts()).toBe(true);
});

test('turning toasts off is saved and read back', () => {
  settings.clear();
  setNotifToasts(false);
  expect(settings.get('notifToasts')).toBe(false);
  expect(getNotifToasts()).toBe(false);
});

test('toasts hide over fullscreen apps by default', () => {
  settings.clear();
  expect(getNotifHideOverFullscreen()).toBe(true);
});
