import { GlobalRegistrator } from '@happy-dom/global-registrator';
import { expect, test, mock } from 'bun:test';

const settings = new Map<string, unknown>();
let manifests: Record<string, string> = {};

mock.module('~/adapter/settings/settings.client', () => ({
  getSetting: <T>(key: string, fallback: T): T => (settings.has(key) ? (settings.get(key) as T) : fallback),
  setSetting: async (key: string, value: unknown) => void settings.set(key, value)
}));

mock.module('~/adapter/plugins/plugins.client', () => ({
  scanPlugins: async () => ({
    root: '/plugins',
    plugins: Object.entries(manifests).map(([dir, manifest]) => ({ dir, manifest }))
  }),
  onPluginsChanged: () => () => {}
}));

if (!GlobalRegistrator.isRegistered) GlobalRegistrator.register();

const nord = {
  id: 'nord',
  name: 'Nord',
  version: '1.0.0',
  contributes: {
    themes: [
      {
        id: 'night',
        label: 'Nord Night',
        kind: 'dark',
        colors: { 'surface-1': '#2e3440' },
        terminal: { foreground: '#d8dee9', ansi: Array(16).fill('#bf616a') }
      },
      { id: 'snow', label: 'Nord Snow', kind: 'light', colors: { 'surface-1': '#eceff4' } }
    ]
  }
};

const load = async () => {
  const theme = await import('~/usecase/util/theme');
  const service = await import('~/usecase/service/plugins.service');
  return { ...theme, ...service };
};

const surface = () => document.documentElement.style.getPropertyValue('--surface-1');

test('each mode lists the built-in theme first, then plugin themes of that kind', async () => {
  const { themeChoices } = await load();
  const plugins = [nord as never];
  expect(themeChoices(plugins, 'dark').map((c) => [c.key, c.label, c.description])).toEqual([
    ['panorama', 'Panorama Dark', 'Built into Panorama.'],
    ['nord:night', 'Nord Night', 'From Nord.']
  ]);
  expect(themeChoices(plugins, 'light').map((c) => c.key)).toEqual(['panorama', 'nord:snow']);
});

test('a theme is only found for its own kind', async () => {
  const { findTheme } = await load();
  expect(findTheme([nord as never], 'nord:night', 'dark')?.label).toBe('Nord Night');
  expect(findTheme([nord as never], 'nord:night', 'light')).toBeNull();
  expect(findTheme([nord as never], 'other:night', 'dark')).toBeNull();
});

test('a chosen plugin theme paints the app and the terminal, and goes away with its plugin', async () => {
  const { initPlugins, initTheme, setThemePref, setThemeChoice, togglePlugin, termTheme, getThemeChoice, usesPluginTheme } =
    await load();
  manifests = { nord: JSON.stringify(nord) };
  await initPlugins();
  initTheme();
  setThemePref('dark');
  expect(usesPluginTheme()).toBe(false);

  setThemeChoice('dark', 'nord:night');
  expect(usesPluginTheme()).toBe(true);
  expect(getThemeChoice('dark')).toBe('nord:night');
  expect(surface()).toBe('#2e3440');
  expect(termTheme.fg).toBe('#d8dee9');
  expect(termTheme.ansi?.get(0xcd3131)).toBe('#bf616a');
  expect(termTheme.ansiBg?.get(0xcd3131)).toBe('#bf616a');

  setThemePref('light');
  expect(surface()).toBe('');
  expect(document.documentElement.dataset.theme).toBe('light');
  expect(termTheme.ansiBg).toBeNull();

  setThemePref('dark');
  togglePlugin('nord');
  expect(getThemeChoice('dark')).toBe('panorama');
  expect(surface()).toBe('');
  expect(termTheme.fg).toBe('#c7d0e0');
  expect(termTheme.ansi).toBeNull();
});
