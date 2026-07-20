import type { Tile } from '~/domain/interfaces/canvas.interface';
import { parseFrontTitle } from '~/usecase/util/noteMeta';

const ALNUM = /[\p{L}\p{N}]/u;
const STAR_PREFIX = /^[\s✳✻✽✶✢❋✱✲✧✦∗*]+/u;

export const stripStarPrefix = (title: string): string => title.replace(STAR_PREFIX, '');

export const hasSpinnerPrefix = (title: string): boolean => {
  const m = title.trim().match(/^(\S{1,3})(\s|$)/u);
  return Boolean(m && !ALNUM.test(m[1]));
};

export const stripSpinner = (title: string): string => {
  let rest = title.trim();
  for (;;) {
    const m = rest.match(/^(\S{1,3})\s+/u);
    if (!m || ALNUM.test(m[1])) break;
    rest = rest.slice(m[0].length);
  }
  if (rest.length <= 3 && !ALNUM.test(rest)) return '';
  return rest.trim();
};

const FALLBACK: Record<string, string> = { term: 'Terminal', note: 'Note' };

export const tileLabel = (tile: Tile): string => {
  if (tile.type === 'note') {
    const t = tile.userTitle || parseFrontTitle(tile.content);
    return t ? `Note - ${t}` : 'Note';
  }
  const osc = tile.oscTitle ? stripSpinner(stripStarPrefix(tile.oscTitle).trim()) : '';
  return tile.userTitle || osc || tile.autoTitle || FALLBACK[tile.type] || tile.type;
};
