import type { Plugin, PluginManifest, PluginSource } from '~/domain/interfaces/plugin.interface';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { isRecord, isText } from '~/usecase/util/pluginFields';
import { parseFonts } from '~/usecase/util/pluginFont';
import { parseLanguages } from '~/usecase/util/pluginLanguage';
import { parseThemes } from '~/usecase/util/pluginTheme';

const DISABLED_KEY = 'pluginsDisabled';
const ID = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const VERSION = /^\d+\.\d+\.\d+([-+][\w.-]+)?$/;

const isOptionalText = (value: unknown): boolean => value === undefined || typeof value === 'string';

export const parseManifest = (dir: string, raw: string | null): PluginManifest | string => {
  if (raw === null) return 'Missing plugin.json';
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return 'plugin.json is not valid JSON';
  }
  if (!isRecord(data)) return 'plugin.json must be a JSON object';

  const { id, name, version, description, author, contributes = {} } = data;
  if (!isText(id) || !ID.test(id)) return '"id" must use lowercase letters, digits and dashes';
  if (id !== dir) return `"id" must match the folder name "${dir}"`;
  if (!isText(name)) return '"name" is required';
  if (!isText(version) || !VERSION.test(version)) return '"version" must look like 1.0.0';
  if (!isOptionalText(description)) return '"description" must be text';
  if (!isOptionalText(author)) return '"author" must be text';
  if (!isRecord(contributes)) return '"contributes" must be an object';
  const themes = parseThemes(contributes.themes);
  if (typeof themes === 'string') return themes;
  const fonts = parseFonts(contributes.fonts);
  if (typeof fonts === 'string') return fonts;
  const languages = parseLanguages(contributes.languages);
  if (typeof languages === 'string') return languages;

  return {
    id,
    name,
    version,
    description: description as string | undefined,
    author: author as string | undefined,
    contributes: { themes, fonts, languages }
  };
};

export const readPlugins = (sources: PluginSource[], disabled: readonly string[]): Plugin[] =>
  sources.map(({ dir, manifest }) => {
    const parsed = parseManifest(dir, manifest);
    return typeof parsed === 'string' ? { dir, error: parsed } : { dir, manifest: parsed, enabled: !disabled.includes(dir) };
  });

export const enabledManifests = (plugins: Plugin[]): PluginManifest[] =>
  plugins.flatMap((p) => ('error' in p || !p.enabled ? [] : [p.manifest]));

export const toggleDisabled = (disabled: readonly string[], id: string): string[] =>
  disabled.includes(id) ? disabled.filter((d) => d !== id) : [...disabled, id];

export const getDisabledPlugins = (): string[] => getSetting<string[]>(DISABLED_KEY, []);

export const setDisabledPlugins = (disabled: string[]): void => void setSetting(DISABLED_KEY, disabled);
