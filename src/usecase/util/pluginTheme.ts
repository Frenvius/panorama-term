import type { PluginTheme } from '~/domain/interfaces/plugin.interface';

export const THEME_COLORS = [
  'surface-1',
  'surface-2',
  'surface-3',
  'border-1',
  'border-2',
  'border-3',
  'text-1',
  'text-strong',
  'text-muted',
  'text-dim',
  'accent',
  'danger',
  'run-glyph',
  'run-fill',
  'run-on-fill',
  'stop-fill',
  'stop-on-fill',
  'fill-hover',
  'fill-press',
  'hover-1',
  'hover-2',
  'hover-3',
  'hover-soft',
  'pill-bg',
  'minimap-bg',
  'shadow-island',
  'shadow-tile',
  'shadow-pop',
  'shadow-modal',
  'shiki-foreground',
  'shiki-token-comment',
  'shiki-token-punctuation',
  'shiki-token-link',
  'shiki-token-keyword',
  'shiki-token-string',
  'shiki-token-string-expression',
  'shiki-token-constant',
  'shiki-token-parameter',
  'shiki-token-function'
];

const ID = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const KNOWN = new Set(THEME_COLORS);

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isText = (value: unknown): value is string => typeof value === 'string' && value.trim() !== '';

const parseColors = (value: unknown, where: string): Record<string, string> | string => {
  if (value === undefined) return {};
  if (!isRecord(value)) return `${where} "colors" must be an object`;
  for (const [key, color] of Object.entries(value)) {
    if (!KNOWN.has(key)) return `${where} has an unknown color "${key}"`;
    if (!isText(color)) return `${where} color "${key}" must be text`;
  }
  return value as Record<string, string>;
};

const parseTerminal = (value: unknown, where: string): PluginTheme['terminal'] | string => {
  if (value === undefined) return {};
  if (!isRecord(value)) return `${where} "terminal" must be an object`;
  const { foreground, cursor, ansi } = value;
  if (foreground !== undefined && !isText(foreground)) return `${where} terminal "foreground" must be text`;
  if (cursor !== undefined && !isText(cursor)) return `${where} terminal "cursor" must be text`;
  if (ansi !== undefined && !(Array.isArray(ansi) && ansi.length === 16 && ansi.every(isText))) {
    return `${where} terminal "ansi" must list 16 colors`;
  }
  return { foreground, cursor, ansi } as PluginTheme['terminal'];
};

const parseTheme = (value: unknown, index: number): PluginTheme | string => {
  const where = `themes[${index}]`;
  if (!isRecord(value)) return `${where} must be an object`;
  const { id, label, kind } = value;
  if (!isText(id) || !ID.test(id)) return `${where} "id" must use lowercase letters, digits and dashes`;
  if (!isText(label)) return `${where} "label" is required`;
  if (kind !== 'dark' && kind !== 'light') return `${where} "kind" must be "dark" or "light"`;
  const colors = parseColors(value.colors, where);
  if (typeof colors === 'string') return colors;
  const terminal = parseTerminal(value.terminal, where);
  if (typeof terminal === 'string') return terminal;
  return { id, label, kind, colors, terminal };
};

export const parseThemes = (value: unknown): PluginTheme[] | string => {
  if (value === undefined) return [];
  if (!Array.isArray(value)) return '"themes" must be a list';
  const themes: PluginTheme[] = [];
  for (const [index, entry] of value.entries()) {
    const theme = parseTheme(entry, index);
    if (typeof theme === 'string') return theme;
    if (themes.some((t) => t.id === theme.id)) return `themes[${index}] repeats the id "${theme.id}"`;
    themes.push(theme);
  }
  return themes;
};
