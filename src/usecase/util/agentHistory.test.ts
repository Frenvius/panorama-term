import { expect, test } from 'bun:test';

import { hasHistory, resumeCommand } from '~/usecase/util/agentHistory';

test('resume command per agent', () => {
  expect(resumeCommand('claude', 'abc')).toBe('claude --resume abc');
  expect(resumeCommand('codex', 'abc')).toBe('codex resume abc');
  expect(resumeCommand('antigravity', 'abc')).toBeNull();
});

test('history only for agents with a resume command', () => {
  expect(hasHistory('kimi')).toBe(true);
  expect(hasHistory('generic')).toBe(false);
  expect(hasHistory(null)).toBe(false);
});
