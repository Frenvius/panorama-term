import type { Highlighter, ThemedToken, BundledLanguage } from 'shiki';

const THEME = 'panorama';

let boot: Promise<Highlighter> | null = null;
let ready: Highlighter | null = null;
let langs: Record<string, unknown> = {};

const start = async (): Promise<Highlighter> => {
  const { createHighlighter, bundledLanguages, createCssVariablesTheme } = await import('shiki');
  langs = bundledLanguages;
  const theme = createCssVariablesTheme({ name: THEME, variablePrefix: '--shiki-', fontStyle: true });
  const hl = await createHighlighter({ themes: [theme], langs: [] });
  ready = hl;
  return hl;
};

export const loadLang = async (lang: string): Promise<string> => {
  const hl = await (boot ??= start());
  if (!(lang in langs)) return 'plaintext';

  const id = lang as BundledLanguage;
  if (!hl.getLoadedLanguages().includes(id)) await hl.loadLanguage(id);

  return id;
};

export const tokenizeNow = (code: string, lang: string): ThemedToken[][] | null => {
  if (!ready || !ready.getLoadedLanguages().includes(lang)) return null;
  return ready.codeToTokens(code, { lang: lang as BundledLanguage, theme: THEME }).tokens;
};

export const highlight = async (code: string, lang: string): Promise<ThemedToken[][]> => {
  const id = await loadLang(lang);
  return (await boot!).codeToTokens(code, { lang: id as BundledLanguage, theme: THEME }).tokens;
};
