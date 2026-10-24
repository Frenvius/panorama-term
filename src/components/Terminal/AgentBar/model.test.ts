import { describe, expect, it } from 'bun:test';

import { effortDesc, contextLabel, modelEfforts, hasOptional1M } from '~/components/Terminal/AgentBar/model';

import type { CatalogModel } from '~/domain/interfaces/pty.interface';

const opus55: CatalogModel = {
  id: 'claude-opus-5-5',
  name: 'Opus 5.5',
  section: 'main',
  contextWindow: 1_000_000,
  efforts: ['low', 'medium', 'high', 'xhigh', 'max'],
  defaultEffort: 'medium'
};

const opus46: CatalogModel = {
  id: 'claude-opus-4-6',
  name: 'Opus 4.6',
  section: 'more',
  contextWindow: 1_000_000,
  efforts: ['low', 'medium', 'high', 'max']
};

const haiku45: CatalogModel = { id: 'claude-haiku-4-5-20251001', name: 'Haiku 4.5', section: 'more', contextWindow: 200_000, efforts: [] };

describe('model helpers', () => {
  it('offers the 1M toggle only where the [1m] variant is optional', () => {
    expect(hasOptional1M('claude-opus-4-6[1m]')).toBe(true);
    expect(hasOptional1M('claude-sonnet-4-6')).toBe(true);
    expect(hasOptional1M('claude-opus-5-5')).toBe(false);
  });

  it('labels the context window from the catalog, or the live window for optional models', () => {
    expect(contextLabel(opus55, false)).toBe('1M');
    expect(contextLabel(opus46, false)).toBe('200k');
    expect(contextLabel(opus46, true)).toBe('1M');
    expect(contextLabel(haiku45, false)).toBe('200k');
    expect(contextLabel(undefined, true)).toBeUndefined();
  });

  it('lists the efforts a model supports, ultracode riding on xhigh', () => {
    expect(modelEfforts(opus55).map((l) => l.id)).toEqual(['low', 'medium', 'high', 'xhigh', 'max', 'ultracode']);
    expect(modelEfforts(opus46).map((l) => l.id)).toEqual(['low', 'medium', 'high', 'max']);
    expect(modelEfforts(haiku45)).toEqual([]);
    expect(modelEfforts(undefined)).toHaveLength(6);
  });

  it('marks the model default effort', () => {
    const [, medium, high] = modelEfforts(opus55);
    expect(effortDesc(medium!, opus55)).toBe('Default');
    expect(effortDesc(high!, opus55)).toBe('Thorough');
  });
});
