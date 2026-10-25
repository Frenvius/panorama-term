import { expect, test } from 'bun:test';

import { mergeClaudeState } from '~/usecase/util/claudeState';

const live = { model: 'opus', effort: 'high', permissionMode: 'plan', contextPercent: 90, contextTokens: 900, costUsd: 2, status: 'idle' };

test('plain update merges over the previous state', () => {
  expect(mergeClaudeState(live, { contextPercent: 91 })).toEqual({ ...live, contextPercent: 91 });
});

test('a reset keeps model, effort and mode but drops the old session fill', () => {
  const next = mergeClaudeState(live, { sessionReset: 'clear', contextPercent: 0, contextTokens: 0 });
  expect(next).toEqual({ model: 'opus', effort: 'high', permissionMode: 'plan', contextPercent: 0, contextTokens: 0 });
});

test('the transcript tracker reset only drops what the tracker reports', () => {
  const afterClear = mergeClaudeState(live, { sessionReset: 'clear', contextPercent: 0, contextTokens: 0 });
  const next = mergeClaudeState(afterClear, { reset: true, mode: 'normal' });
  expect(next.model).toBe('opus');
  expect(next.effort).toBe('high');
  expect(next.contextPercent).toBe(0);
  expect(next.contextTokens).toBeUndefined();
  expect(next.mode).toBe('normal');
});
