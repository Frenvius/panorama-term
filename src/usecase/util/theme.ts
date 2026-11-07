import type { PluginChoice, PluginManifest, PluginTheme, ThemeKind } from '~/domain/interfaces/plugin.interface';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { enabledPlugins, PLUGINS_EVENT } from '~/usecase/service/plugins.service';

export type ThemePref = 'system' | 'dark' | 'light';
type Resolved = ThemeKind;

export const THEME_KEY = 'theme';
export const THEME_EVENT = 'panorama-theme';
export const BUILTIN_THEME = 'panorama';

const CHOICE_KEYS: Record<ThemeKind, string> = { dark: 'themeDark', light: 'themeLight' };
const BUILTIN_LABELS: Record<ThemeKind, string> = { dark: 'Panorama Dark', light: 'Panorama Light' };

type AnsiMap = Map<number, string> | null;

export const termTheme: { bg: string; fg: string; cursor: string; ansi: AnsiMap; ansiBg: AnsiMap } = {
  bg: '#0b0e14',
  fg: '#c7d0e0',
  cursor: 'rgba(199,208,224,0.65)',
  ansi: null,
  ansiBg: null
};
export const themeInk = { dot: '255, 255, 255', line: '#ffffff' };

const BASE16 = [
  0x000000, 0xcd3131, 0x0dbc79, 0xe5e510, 0x2472c8, 0xbc3fbc, 0x11a8cd, 0xe5e5e5, 0x666666,
  0xf14c4c, 0x23d18b, 0xf5f543, 0x3b8eea, 0xd670d6, 0x29b8db, 0xe5e5e5
];

const LIGHT16 = [
  '#242424', '#c01c28', '#1a7f37', '#9a6700', '#1a5fb4', '#9c2f9c', '#0b7285', '#5c5c5c', '#727272',
  '#d01a1a', '#1a7f43', '#7d6608', '#2563c9', '#a626a6', '#106b7d', '#1a1a1a'
];

export const ansiMap = (colors: string[]): Map<number, string> => new Map(BASE16.map((rgb, i) => [rgb, colors[i]]));

const DARK = {
  bg: '#0b0e14',
  fg: '#c7d0e0',
  cursor: 'rgba(199,208,224,0.65)',
  dot: '255, 255, 255',
  line: '#ffffff'
};

const LIGHT = {
  bg: '#f4f4f4',
  fg: '#202020',
  cursor: 'rgba(32,32,32,0.55)',
  dot: '20, 20, 20',
  line: '#303030'
};

export const themeChoices = (plugins: PluginManifest[], kind: ThemeKind): PluginChoice[] => [
  { key: BUILTIN_THEME, label: BUILTIN_LABELS[kind], description: 'Built into Panorama.' },
  ...plugins.flatMap((plugin) =>
    plugin.contributes.themes
      .filter((theme) => theme.kind === kind)
      .map((theme) => ({ key: `${plugin.id}:${theme.id}`, label: theme.label, description: `From ${plugin.name}.` }))
  )
];

export const findTheme = (plugins: PluginManifest[], key: string, kind: ThemeKind): PluginTheme | null => {
  const [pluginId, themeId] = key.split(':');
  const theme = plugins.find((p) => p.id === pluginId)?.contributes.themes.find((t) => t.id === themeId);
  return theme?.kind === kind ? theme : null;
};

const savedChoice = (kind: ThemeKind): string => getSetting(CHOICE_KEYS[kind], BUILTIN_THEME);

export const getThemeChoice = (kind: ThemeKind): string => {
  const key = savedChoice(kind);
  return findTheme(enabledPlugins(), key, kind) ? key : BUILTIN_THEME;
};

export const usesPluginTheme = (): boolean =>
  savedChoice('dark') !== BUILTIN_THEME || savedChoice('light') !== BUILTIN_THEME;

const mql = () => window.matchMedia('(prefers-color-scheme: light)');

export const resolveTheme = (pref: ThemePref): Resolved =>
  pref === 'system' ? (mql().matches ? 'light' : 'dark') : pref;

let painted: string[] = [];
let applied = '';

const paint = (colors: Record<string, string>): void => {
  const style = document.documentElement.style;
  for (const name of painted) style.removeProperty(`--${name}`);
  for (const [name, value] of Object.entries(colors)) style.setProperty(`--${name}`, value);
  painted = Object.keys(colors);
};

export const applyTheme = (pref: ThemePref): void => {
  const resolved = resolveTheme(pref);
  const theme = findTheme(enabledPlugins(), savedChoice(resolved), resolved);
  const signature = JSON.stringify([resolved, theme]);
  if (signature === applied) return;
  applied = signature;

  const p = resolved === 'light' ? LIGHT : DARK;
  document.documentElement.dataset.theme = resolved;
  paint(theme?.colors ?? {});
  termTheme.bg = p.bg;
  termTheme.fg = theme?.terminal.foreground ?? p.fg;
  termTheme.cursor = theme?.terminal.cursor ?? p.cursor;
  const palette = theme?.terminal.ansi ? ansiMap(theme.terminal.ansi) : null;
  termTheme.ansi = palette ?? (resolved === 'light' ? ansiMap(LIGHT16) : null);
  termTheme.ansiBg = palette;
  themeInk.dot = p.dot;
  themeInk.line = p.line;
  window.dispatchEvent(new Event(THEME_EVENT));
};

export const getThemePref = (): ThemePref => getSetting(THEME_KEY, 'system' as ThemePref);

export const setThemePref = (pref: ThemePref): void => {
  void setSetting(THEME_KEY, pref);
  applyTheme(pref);
};

export const setThemeChoice = (kind: ThemeKind, key: string): void => {
  void setSetting(CHOICE_KEYS[kind], key);
  applyTheme(getThemePref());
};

let bound = false;

export const initTheme = (): void => {
  applyTheme(getThemePref());
  if (bound) return;
  bound = true;
  const reapply = () => applyTheme(getThemePref());
  mql().addEventListener('change', () => {
    if (getThemePref() === 'system') reapply();
  });
  window.addEventListener(PLUGINS_EVENT, reapply);
};
