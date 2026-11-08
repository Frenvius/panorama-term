import type { PluginFont, PluginFontFile } from '~/domain/interfaces/plugin.interface';

import { isInside, isRecord } from '~/usecase/util/pluginFields';

const FAMILY = /^[A-Za-z0-9][A-Za-z0-9 ._-]*$/;
const FONT_FILE = /\.(woff2?|ttf|otf)$/i;
const WEIGHT = /^[1-9]00$/;

const parseFile = (value: unknown, where: string): PluginFontFile | string => {
  const file = typeof value === 'string' ? { path: value } : value;
  if (!isRecord(file)) return `${where} must be a path or an object`;
  const { path, weight = '400', style = 'normal' } = file;
  if (typeof path !== 'string' || !isInside(path) || !FONT_FILE.test(path)) {
    return `${where} must be a .woff2, .woff, .ttf or .otf file inside the plugin`;
  }
  const w = String(weight);
  if (!WEIGHT.test(w)) return `${where} "weight" must be 100 to 900`;
  if (style !== 'normal' && style !== 'italic') return `${where} "style" must be "normal" or "italic"`;
  return { path, weight: w, style };
};

const parseFont = (value: unknown, index: number): PluginFont | string => {
  const where = `fonts[${index}]`;
  if (!isRecord(value)) return `${where} must be an object`;
  const { family, monospace = false, files } = value;
  if (typeof family !== 'string' || !FAMILY.test(family)) {
    return `${where} "family" must use letters, digits, spaces, dots, dashes or underscores`;
  }
  if (typeof monospace !== 'boolean') return `${where} "monospace" must be true or false`;
  if (!Array.isArray(files) || files.length === 0) return `${where} "files" must list at least one file`;
  const parsed: PluginFontFile[] = [];
  for (const [i, entry] of files.entries()) {
    const file = parseFile(entry, `${where} files[${i}]`);
    if (typeof file === 'string') return file;
    parsed.push(file);
  }
  return { family, monospace, files: parsed };
};

export const parseFonts = (value: unknown): PluginFont[] | string => {
  if (value === undefined) return [];
  if (!Array.isArray(value)) return '"fonts" must be a list';
  const fonts: PluginFont[] = [];
  for (const [index, entry] of value.entries()) {
    const font = parseFont(entry, index);
    if (typeof font === 'string') return font;
    if (fonts.some((f) => f.family === font.family)) return `fonts[${index}] repeats the family "${font.family}"`;
    fonts.push(font);
  }
  return fonts;
};
