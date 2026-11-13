import { GlobalRegistrator } from '@happy-dom/global-registrator';
import { expect, test, mock, afterAll } from 'bun:test';

const settings = new Map<string, unknown>();
let manifests: Record<string, string> = {};

mock.module('~/adapter/settings/settings.client', () => ({
  getSetting: <T>(key: string, fallback: T): T => (settings.has(key) ? (settings.get(key) as T) : fallback),
  setSetting: async (key: string, value: unknown) => void settings.set(key, value)
}));

const files = new Map<string, ArrayBuffer | string>();

const file = async (path: string) => {
  const content = files.get(path);
  if (content === undefined) throw new Error(`no file ${path}`);
  return content;
};

mock.module('~/adapter/fs/fs.client', () => ({ readFileBytes: file, readTextFile: file }));

mock.module('~/adapter/plugins/plugins.client', () => ({
  scanPlugins: async () => ({
    root: '/plugins',
    plugins: Object.entries(manifests).map(([dir, manifest]) => ({ dir, manifest }))
  }),
  onPluginsChanged: () => () => {}
}));

const ownDom = !GlobalRegistrator.isRegistered;
if (ownDom) GlobalRegistrator.register();
afterAll(async () => {
  if (ownDom) await GlobalRegistrator.unregister();
});

const added: { family: string; weight?: string }[] = [];
class FakeFontFace {
  constructor(
    readonly family: string,
    readonly source: ArrayBuffer,
    readonly descriptors: { weight?: string }
  ) {}
  load = async () => this;
}
Object.assign(globalThis, { FontFace: FakeFontFace });
Object.defineProperty(document, 'fonts', {
  value: { add: (face: FakeFontFace) => added.push({ family: face.family, weight: face.descriptors.weight }) }
});
HTMLCanvasElement.prototype.getContext = (() => ({ font: '', measureText: () => ({ width: 800 }) })) as never;

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

const iosevka = {
  id: 'iosevka',
  name: 'Iosevka',
  version: '1.0.0',
  contributes: {
    fonts: [
      { family: 'Iosevka Term', monospace: true, files: ['regular.woff2', { path: 'bold.woff2', weight: 700 }] },
      { family: 'Iosevka Aile', files: ['aile.woff2'] }
    ]
  }
};

const load = async () => {
  const theme = await import('~/usecase/util/theme');
  const fonts = await import('~/usecase/util/fonts');
  const highlight = await import('~/usecase/service/highlight');
  const service = await import('~/usecase/service/plugins.service');
  return { ...theme, ...fonts, ...highlight, ...service };
};

const cssVar = (name: string) => document.documentElement.style.getPropertyValue(name);

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

test('only monospace fonts are offered for code and the terminal', async () => {
  const { fontChoices, findFont } = await load();
  const fonts = [
    { family: 'Iosevka Term', monospace: true, files: [] },
    { family: 'Iosevka Aile', monospace: false, files: [] }
  ];
  const parsed = [{ ...iosevka, contributes: { themes: [], fonts } }] as never[];
  expect(fontChoices(parsed, 'ui').map((c) => c.label)).toEqual(['Default', 'Iosevka Term', 'Iosevka Aile']);
  expect(fontChoices(parsed, 'terminal').map((c) => [c.key, c.description])).toEqual([
    ['default', 'Hack.'],
    ['iosevka:Iosevka Term', 'From Iosevka.']
  ]);
  expect(findFont(parsed, 'iosevka:Iosevka Aile', 'code')).toBeNull();
  expect(findFont(parsed, 'iosevka:Iosevka Aile', 'ui')?.font.family).toBe('Iosevka Aile');
});

test('a chosen terminal font is loaded from the plugin, measured, and dropped with its plugin', async () => {
  const { initPlugins, initFonts, setFontChoice, togglePlugin, termMetrics, usesPluginFont, getFontChoice } = await load();
  manifests = { iosevka: JSON.stringify(iosevka) };
  files.set('/plugins/iosevka/regular.woff2', new ArrayBuffer(4));
  files.set('/plugins/iosevka/bold.woff2', new ArrayBuffer(4));
  await initPlugins();
  await initFonts();
  expect(usesPluginFont()).toBe(false);
  expect(termMetrics.cellW).toBe(7.23);

  await setFontChoice('terminal', 'iosevka:Iosevka Term');
  expect(added).toEqual([
    { family: 'Iosevka Term', weight: '400' },
    { family: 'Iosevka Term', weight: '700' }
  ]);
  expect(cssVar('--font-terminal')).toBe("'Iosevka Term', monospace");
  expect(termMetrics.cellW).toBe(8);
  expect(getFontChoice('terminal')).toBe('iosevka:Iosevka Term');

  // aile.woff2 is missing: a font that cannot load is not applied, the rest stays
  await setFontChoice('ui', 'iosevka:Iosevka Aile');
  expect(cssVar('--font-ui')).toBe('');
  expect(termMetrics.cellW).toBe(8);

  togglePlugin('iosevka');
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(cssVar('--font-terminal')).toBe('');
  expect(termMetrics.cellW).toBe(7.23);
});

const demo = {
  id: 'demo',
  name: 'Demo',
  version: '1.0.0',
  contributes: {
    languages: [
      { id: 'demo', extensions: ['.dmo'], grammar: 'demo.tmLanguage.json' },
      { id: 'groovy', extensions: ['.jenkins'] }
    ]
  }
};

const grammar = {
  scopeName: 'source.demo',
  patterns: [{ match: '\\bhello\\b', name: 'keyword.control.demo' }]
};

test('plugin grammars color their files, extensions map to known languages, both go away with the plugin', async () => {
  const { initPlugins, togglePlugin, loadLang, highlight } = await load();
  manifests = { demo: JSON.stringify(demo) };
  files.set('/plugins/demo/demo.tmLanguage.json', JSON.stringify(grammar));
  await initPlugins();

  const lang = await loadLang('dmo');
  expect(lang).toStartWith('demo/demo#');
  const [line] = await highlight('hello world', 'dmo');
  expect(line[0]).toMatchObject({ content: 'hello', color: 'var(--shiki-token-keyword)' });
  expect(await loadLang('demo')).toBe(lang);
  expect(await loadLang('jenkins')).toBe('groovy');

  togglePlugin('demo');
  expect(await loadLang('dmo')).toBe('plaintext');
  expect(await loadLang('jenkins')).toBe('plaintext');
  expect(await loadLang('ts')).toBe('ts');
});

test('a grammar that cannot be read leaves the file uncolored', async () => {
  const { initPlugins, togglePlugin, loadLang } = await load();
  manifests = { demo: JSON.stringify(demo) };
  files.delete('/plugins/demo/demo.tmLanguage.json');
  await initPlugins();
  togglePlugin('demo');
  expect(await loadLang('dmo')).toBe('plaintext');
});

test('a theme background is read from the plugin and goes away with the theme', async () => {
  const { initPlugins, setThemePref, setThemeChoice, getBackdrop } = await load();
  const cats = {
    id: 'cats',
    name: 'Cats',
    version: '1.0.0',
    contributes: {
      themes: [{ id: 'night', label: 'Cats Dark', kind: 'dark', background: { image: 'bg/cats.svg', opacity: 0.5 } }]
    }
  };
  manifests = { cats: JSON.stringify(cats) };
  files.set('/plugins/cats/bg/cats.svg', new TextEncoder().encode('<svg xmlns="http://www.w3.org/2000/svg"/>').buffer);
  await initPlugins();
  setThemePref('dark');
  setThemeChoice('dark', 'cats:night');
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(getBackdrop()).toMatchObject({ opacity: 0.5 });
  expect(getBackdrop()?.url).toStartWith('blob:');

  setThemeChoice('dark', 'panorama');
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(getBackdrop()).toBeNull();
});
