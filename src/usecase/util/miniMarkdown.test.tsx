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

test('drops blank lines', () => {
  expect(miniMarkdown('a\n\n\nb').length).toBe(2);
});
