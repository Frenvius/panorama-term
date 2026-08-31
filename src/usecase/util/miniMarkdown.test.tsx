import { test, expect } from 'bun:test';

import { miniMarkdown } from '~/usecase/util/miniMarkdown';

test('splits fences, bullets and text', () => {
  const blocks = miniMarkdown('intro\n\n- one\n- two\n\n```\ncd /tmp\nbun dev\n```\ntail');
  expect(blocks.map((b) => b.kind)).toEqual(['text', 'bullet', 'bullet', 'code', 'text']);
  expect(blocks[3].body).toBe('cd /tmp\nbun dev');
});

test('unclosed fence still emits its code', () => {
  const blocks = miniMarkdown('run:\n```\nbun dev');
  expect(blocks.map((b) => b.kind)).toEqual(['text', 'code']);
  expect(blocks[1].body).toBe('bun dev');
});

test('folds a pipe table into one block', () => {
  const raw = '## Context Usage\n| Category | Tokens |\n|----------|--------|\n| System prompt | 3.6k |\n| Messages | 3.2k |\ntail';
  const blocks = miniMarkdown(raw);
  expect(blocks.map((b) => b.kind)).toEqual(['text', 'table', 'text']);
});

test('a pipe line without a divider stays text', () => {
  const blocks = miniMarkdown('| not | a table |\nplain');
  expect(blocks.map((b) => b.kind)).toEqual(['text', 'text']);
});

test('marks paths, urls and hashes inside text', () => {
  const [block] = miniMarkdown('see src/main.rs and https://x.dev at 9e603c0 ok');
  const marked = JSON.stringify(block.body);
  expect((marked.match(/"em"/g) ?? []).length).toBe(3);
  expect(marked).toContain('src/main.rs');
});

test('drops blank lines', () => {
  expect(miniMarkdown('a\n\n\nb').length).toBe(2);
});
