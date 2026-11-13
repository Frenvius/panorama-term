import type { Highlighter, ThemedToken, BundledLanguage, LanguageRegistration } from 'shiki';
import type { PluginLanguage, PluginManifest } from '~/domain/interfaces/plugin.interface';

import { readTextFile } from '~/adapter/fs/fs.client';
import { findLanguage } from '~/usecase/util/pluginLanguage';
import { enabledPlugins, getPluginsState, PLUGINS_EVENT } from '~/usecase/service/plugins.service';

const THEME = 'panorama';
const MAX_HIGHLIGHT_CHARS = 400_000;

let boot: Promise<Highlighter> | null = null;
let ready: Highlighter | null = null;
let langs: Record<string, unknown> = {};

let grammars = new Map<string, Promise<string | null>>();
let generation = 0;

const loadGrammar = (hl: Highlighter, plugin: PluginManifest, language: PluginLanguage): Promise<string | null> => {
  const name = `${plugin.id}/${language.id}#${generation}`;
  let done = grammars.get(name);
  if (!done) {
    done = readTextFile(`${getPluginsState().root}/${plugin.id}/${language.grammar}`)
      .then(async (text) => {
        const grammar = JSON.parse(text) as LanguageRegistration;
        if (typeof grammar.scopeName !== 'string') return null;
        await hl.loadLanguage({ ...grammar, name });
        return name;
      })
      .catch(() => null);
    grammars.set(name, done);
  }
  return done;
};

const start = async (): Promise<Highlighter> => {
  window.addEventListener(PLUGINS_EVENT, () => {
    grammars = new Map();
    generation++;
  });
  const { createHighlighter, bundledLanguages, createCssVariablesTheme } = await import('shiki');
  langs = bundledLanguages;
  const theme = createCssVariablesTheme({ name: THEME, variablePrefix: '--shiki-', fontStyle: true });
  const hl = await createHighlighter({ themes: [theme], langs: [] });
  ready = hl;
  return hl;
};

export const loadLang = async (lang: string): Promise<string> => {
  const hl = await (boot ??= start());
  const found = findLanguage(enabledPlugins(), lang);
  if (found?.language.grammar) return (await loadGrammar(hl, found.plugin, found.language)) ?? 'plaintext';

  const id = (found?.language.id ?? lang) as BundledLanguage;
  if (!(id in langs)) return 'plaintext';
  if (!hl.getLoadedLanguages().includes(id)) await hl.loadLanguage(id);

  return id;
};

export const tokenizeNow = (code: string, lang: string): ThemedToken[][] | null => {
  if (!ready || !ready.getLoadedLanguages().includes(lang)) return null;
  return ready.codeToTokens(code, { lang: lang as BundledLanguage, theme: THEME }).tokens;
};

export const highlight = async (code: string, lang: string): Promise<ThemedToken[][]> => {
  const id = await loadLang(code.length > MAX_HIGHLIGHT_CHARS ? 'plaintext' : lang);
  return (await boot!).codeToTokens(code, { lang: id as BundledLanguage, theme: THEME }).tokens;
};
