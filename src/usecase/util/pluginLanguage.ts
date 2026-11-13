import type { PluginLanguage, PluginManifest } from '~/domain/interfaces/plugin.interface';

import { isInside, isRecord } from '~/usecase/util/pluginFields';

const ID = /^[a-z0-9][a-z0-9+#.-]*$/;
const EXTENSION = /^\.[A-Za-z0-9_+-]+$/;
const GRAMMAR = /\.json$/i;

const parseLanguage = (value: unknown, index: number): PluginLanguage | string => {
  const where = `languages[${index}]`;
  if (!isRecord(value)) return `${where} must be an object`;
  const { id, extensions = [], grammar } = value;
  if (typeof id !== 'string' || !ID.test(id)) return `${where} "id" must use lowercase letters, digits and + # . -`;
  if (!Array.isArray(extensions) || !extensions.every((e) => typeof e === 'string' && EXTENSION.test(e))) {
    return `${where} "extensions" must list extensions like ".ext"`;
  }
  if (grammar !== undefined && (typeof grammar !== 'string' || !isInside(grammar) || !GRAMMAR.test(grammar))) {
    return `${where} "grammar" must be a .json file inside the plugin`;
  }
  if (grammar === undefined && extensions.length === 0) return `${where} needs a "grammar" or "extensions"`;
  return { id, extensions: extensions.map((e: string) => e.toLowerCase()), grammar };
};

export const parseLanguages = (value: unknown): PluginLanguage[] | string => {
  if (value === undefined) return [];
  if (!Array.isArray(value)) return '"languages" must be a list';
  const languages: PluginLanguage[] = [];
  for (const [index, entry] of value.entries()) {
    const language = parseLanguage(entry, index);
    if (typeof language === 'string') return language;
    if (languages.some((l) => l.id === language.id)) return `languages[${index}] repeats the id "${language.id}"`;
    languages.push(language);
  }
  return languages;
};

export const findLanguage = (
  plugins: PluginManifest[],
  lang: string
): { plugin: PluginManifest; language: PluginLanguage } | null => {
  const key = lang.toLowerCase();
  for (const plugin of plugins) {
    for (const language of plugin.contributes.languages) {
      if (language.extensions.includes(`.${key}`) || (language.grammar && language.id === key)) {
        return { plugin, language };
      }
    }
  }
  return null;
};
