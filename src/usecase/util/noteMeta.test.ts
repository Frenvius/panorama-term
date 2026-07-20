import { expect, test } from 'bun:test';

import { stripFrontmatter, parseFrontTitle, applyFrontTitle } from '~/usecase/util/noteMeta';

test('parses title from frontmatter', () => {
  expect(parseFrontTitle('---\ntitle: Meu Plano\n---\ncorpo')).toBe('Meu Plano');
  expect(parseFrontTitle('corpo sem fm')).toBe('');
  expect(parseFrontTitle('')).toBe('');
});

test('strips frontmatter leaving body', () => {
  expect(stripFrontmatter('---\ntitle: X\n---\n- [ ] item')).toBe('- [ ] item');
  expect(stripFrontmatter('- [ ] item')).toBe('- [ ] item');
});

test('applies and replaces title, preserving body', () => {
  const a = applyFrontTitle('- [ ] item', 'Plano');
  expect(a).toBe('---\ntitle: Plano\n---\n- [ ] item');
  expect(applyFrontTitle(a, 'Novo')).toBe('---\ntitle: Novo\n---\n- [ ] item');
  expect(applyFrontTitle(a, '')).toBe('- [ ] item');
});

test('quotes titles with special chars and round-trips', () => {
  const raw = applyFrontTitle('body', 'ratio 16:9 #build');
  expect(parseFrontTitle(raw)).toBe('ratio 16:9 #build');
  expect(stripFrontmatter(raw)).toBe('body');
});
