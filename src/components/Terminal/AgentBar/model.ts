import type { CatalogModel } from '~/domain/interfaces/pty.interface';

import { EFFORT_LEVELS, OPTIONAL_1M_MODELS } from '~/components/Terminal/AgentBar/constants';

type EffortLevel = (typeof EFFORT_LEVELS)[number];

export const baseModelId = (id: string): string => id.replace(/\[[^\]]*\]/, '');

export const hasOptional1M = (id: string): boolean => OPTIONAL_1M_MODELS.includes(baseModelId(id));

export const findModel = (catalog: CatalogModel[], id: string): CatalogModel | undefined =>
  catalog.find((m) => m.id === baseModelId(id));

export const contextLabel = (entry: CatalogModel | undefined, is1M: boolean): string | undefined => {
  if (!entry) return undefined;
  if (hasOptional1M(entry.id)) return is1M ? '1M' : '200k';
  if (!entry.contextWindow) return is1M ? '1M' : undefined;
  return entry.contextWindow >= 1_000_000 ? '1M' : `${Math.round(entry.contextWindow / 1000)}k`;
};

export const modelEfforts = (entry: CatalogModel | undefined): EffortLevel[] => {
  const supported = entry?.efforts;
  if (!supported) return [...EFFORT_LEVELS];
  return EFFORT_LEVELS.filter((l) => supported.includes(l.id) || (l.id === 'ultracode' && supported.includes('xhigh')));
};

export const effortDesc = (level: EffortLevel, entry: CatalogModel | undefined): string =>
  level.id === entry?.defaultEffort ? 'Default' : level.desc;
