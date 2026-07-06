import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type ThemePref = 'system' | 'dark' | 'light';
type Resolved = 'dark' | 'light';

export const THEME_KEY = 'theme';
export const THEME_EVENT = 'panorama-theme';

type AnsiMap = Map<number, string> | null;

export const termTheme: { bg: string; fg: string; cursor: string; ansi: AnsiMap } = {
  bg: '#0b0e14',
  fg: '#c7d0e0',
  cursor: 'rgba(199,208,224,0.65)',
  ansi: null
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

const lightAnsi = (): AnsiMap => new Map(BASE16.map((rgb, i) => [rgb, LIGHT16[i]]));

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

const mql = () => window.matchMedia('(prefers-color-scheme: light)');

export const resolveTheme = (pref: ThemePref): Resolved =>
  pref === 'system' ? (mql().matches ? 'light' : 'dark') : pref;

export const applyTheme = (pref: ThemePref): void => {
  const resolved = resolveTheme(pref);
  const p = resolved === 'light' ? LIGHT : DARK;
  document.documentElement.dataset.theme = resolved;
  termTheme.bg = p.bg;
  termTheme.fg = p.fg;
  termTheme.cursor = p.cursor;
  termTheme.ansi = resolved === 'light' ? lightAnsi() : null;
  themeInk.dot = p.dot;
  themeInk.line = p.line;
  window.dispatchEvent(new Event(THEME_EVENT));
};

export const getThemePref = (): ThemePref => getSetting(THEME_KEY, 'system' as ThemePref);

export const setThemePref = (pref: ThemePref): void => {
  void setSetting(THEME_KEY, pref);
  applyTheme(pref);
};

let bound = false;

export const initTheme = (): void => {
  applyTheme(getThemePref());
  if (bound) return;
  bound = true;
  mql().addEventListener('change', () => {
    if (getThemePref() === 'system') applyTheme('system');
  });
};
