import type { FontUse, PluginChoice, PluginFont, PluginManifest } from '~/domain/interfaces/plugin.interface';

import { readFileBytes } from '~/adapter/fs/fs.client';
import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { THEME_EVENT } from '~/usecase/util/theme';
import { enabledPlugins, getPluginsState, PLUGINS_EVENT } from '~/usecase/service/plugins.service';

export const DEFAULT_FONT = 'default';
export const DEFAULT_CELL_W = 7.23;
export const FONT_USES: FontUse[] = ['ui', 'code', 'terminal'];

const KEYS: Record<FontUse, string> = { ui: 'fontUi', code: 'fontCode', terminal: 'fontTerminal' };
const VARS: Record<FontUse, string> = { ui: '--font-ui', code: '--font-mono', terminal: '--font-terminal' };
const DEFAULTS: Record<FontUse, string> = { ui: 'System font.', code: 'JetBrains Mono.', terminal: 'Hack.' };

export const termMetrics = { cellW: DEFAULT_CELL_W };

const fits = (font: PluginFont, use: FontUse): boolean => use === 'ui' || font.monospace;

export const fontChoices = (plugins: PluginManifest[], use: FontUse): PluginChoice[] => [
  { key: DEFAULT_FONT, label: 'Default', description: DEFAULTS[use] },
  ...plugins.flatMap((plugin) =>
    plugin.contributes.fonts
      .filter((font) => fits(font, use))
      .map((font) => ({ key: `${plugin.id}:${font.family}`, label: font.family, description: `From ${plugin.name}.` }))
  )
];

export const findFont = (
  plugins: PluginManifest[],
  key: string,
  use: FontUse
): { plugin: PluginManifest; font: PluginFont } | null => {
  const at = key.indexOf(':');
  const plugin = plugins.find((p) => p.id === key.slice(0, at));
  const font = plugin?.contributes.fonts.find((f) => f.family === key.slice(at + 1));
  return plugin && font && fits(font, use) ? { plugin, font } : null;
};

const savedChoice = (use: FontUse): string => getSetting(KEYS[use], DEFAULT_FONT);

export const getFontChoice = (use: FontUse): string => {
  const key = savedChoice(use);
  return findFont(enabledPlugins(), key, use) ? key : DEFAULT_FONT;
};

export const usesPluginFont = (): boolean => FONT_USES.some((use) => savedChoice(use) !== DEFAULT_FONT);

const loaded = new Map<string, Promise<void>>();

const loadFont = (root: string, plugin: PluginManifest, font: PluginFont): Promise<void> => {
  const key = `${plugin.id}:${font.family}`;
  let done = loaded.get(key);
  if (!done) {
    done = Promise.all(
      font.files.map(async ({ path, weight, style }) => {
        const bytes = await readFileBytes(`${root}/${plugin.id}/${path}`);
        document.fonts.add(await new FontFace(font.family, bytes, { weight, style }).load());
      })
    ).then(() => undefined);
    loaded.set(key, done);
  }
  return done;
};

const measureCell = (family: string): number => {
  const ctx = document.createElement('canvas').getContext('2d');
  if (!ctx) return DEFAULT_CELL_W;
  ctx.font = `12px '${family}'`;
  return ctx.measureText('M'.repeat(100)).width / 100 || DEFAULT_CELL_W;
};

type Picked = (readonly [FontUse, { plugin: PluginManifest; font: PluginFont } | null])[];

const signatureOf = (picked: Picked): string =>
  JSON.stringify(picked.map(([use, found]) => [use, found?.plugin.id ?? null, found?.font ?? null]));

let applied = signatureOf(FONT_USES.map((use) => [use, null] as const));
let run = 0;

export const applyFonts = async (): Promise<void> => {
  const plugins = enabledPlugins();
  const picked: Picked = FONT_USES.map((use) => [use, findFont(plugins, savedChoice(use), use)] as const);
  const signature = signatureOf(picked);
  if (signature === applied) return;
  const mine = ++run;

  const root = getPluginsState().root;
  const results = await Promise.allSettled(
    picked.map(([, found]) => (found && root ? loadFont(root, found.plugin, found.font) : Promise.resolve()))
  );
  if (mine !== run) return;
  applied = signature;

  const style = document.documentElement.style;
  picked.forEach(([use, found], i) => {
    const ok = found && results[i].status === 'fulfilled';
    if (ok) style.setProperty(VARS[use], `'${found.font.family}', ${use === 'ui' ? 'sans-serif' : 'monospace'}`);
    else style.removeProperty(VARS[use]);
    if (use === 'terminal') termMetrics.cellW = ok ? measureCell(found.font.family) : DEFAULT_CELL_W;
  });
  window.dispatchEvent(new Event(THEME_EVENT));
};

export const setFontChoice = (use: FontUse, key: string): Promise<void> => {
  void setSetting(KEYS[use], key);
  return applyFonts();
};

let bound = false;

export const initFonts = (): Promise<void> => {
  if (!bound) {
    bound = true;
    window.addEventListener(PLUGINS_EVENT, () => void applyFonts());
  }
  return applyFonts();
};
