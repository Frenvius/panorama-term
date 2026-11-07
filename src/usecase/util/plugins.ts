import type { Plugin, PluginManifest, PluginSource } from '~/domain/interfaces/plugin.interface';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';

const DISABLED_KEY = 'pluginsDisabled';
const ID = /^[a-z0-9]+(-[a-z0-9]+)*$/;
const VERSION = /^\d+\.\d+\.\d+([-+][\w.-]+)?$/;

const isText = (value: unknown): value is string => typeof value === 'string' && value.trim() !== '';
const isOptionalText = (value: unknown): boolean => value === undefined || typeof value === 'string';

export const parseManifest = (dir: string, raw: string | null): PluginManifest | string => {
  if (raw === null) return 'Missing plugin.json';
  let data: unknown;
  try {
    data = JSON.parse(raw);
  } catch {
    return 'plugin.json is not valid JSON';
  }
  if (typeof data !== 'object' || data === null || Array.isArray(data)) return 'plugin.json must be a JSON object';

  const { id, name, version, description, author } = data as Record<string, unknown>;
  if (!isText(id) || !ID.test(id)) return '"id" must use lowercase letters, digits and dashes';
  if (id !== dir) return `"id" must match the folder name "${dir}"`;
  if (!isText(name)) return '"name" is required';
  if (!isText(version) || !VERSION.test(version)) return '"version" must look like 1.0.0';
  if (!isOptionalText(description)) return '"description" must be text';
  if (!isOptionalText(author)) return '"author" must be text';

  return { id, name, version, description: description as string | undefined, author: author as string | undefined };
};

export const readPlugins = (sources: PluginSource[], disabled: readonly string[]): Plugin[] =>
  sources.map(({ dir, manifest }) => {
    const parsed = parseManifest(dir, manifest);
    return typeof parsed === 'string' ? { dir, error: parsed } : { dir, manifest: parsed, enabled: !disabled.includes(dir) };
  });

export const toggleDisabled = (disabled: readonly string[], id: string): string[] =>
  disabled.includes(id) ? disabled.filter((d) => d !== id) : [...disabled, id];

export const getDisabledPlugins = (): string[] => getSetting<string[]>(DISABLED_KEY, []);

export const setDisabledPlugins = (disabled: string[]): void => void setSetting(DISABLED_KEY, disabled);
