import { test, expect } from 'bun:test';

import { shellSummary } from '~/usecase/util/shellSummary';

test('pwsh: last command with its last output line', () => {
  const lines = ['PS D:\\repo> cargo test', 'running 42 tests', 'test result: FAILED. 41 passed; 1 failed', '', 'PS D:\\repo> ', ''];
  expect(shellSummary(lines)).toEqual({ cmd: 'cargo test', out: 'test result: FAILED. 41 passed; 1 failed' });
});

test('command still running has no prompt after it', () => {
  const lines = ['PS D:\\repo> bun run dev', '  VITE ready in 412 ms', '  Local: http://localhost:5173/'];
  expect(shellSummary(lines)).toEqual({ cmd: 'bun run dev', out: 'Local: http://localhost:5173/' });
});

test('bash prompt', () => {
  const lines = ['me@box:~/repo$ git status', 'nothing to commit, working tree clean', 'me@box:~/repo$ '];
  expect(shellSummary(lines)).toEqual({ cmd: 'git status', out: 'nothing to commit, working tree clean' });
});

test('skips empty prompts back to the last real command', () => {
  const lines = ['❯ ls', 'a b c', '❯ ', '❯ '];
  expect(shellSummary(lines)).toEqual({ cmd: 'ls', out: 'a b c' });
});

test('two-line prompt header is not output', () => {
  const lines = ['~ on (us-east-1)', '❯ git --version', 'git version 2.47.0', '', '~ on (us-east-1)', '❯ '];
  expect(shellSummary(lines)).toEqual({ cmd: 'git --version', out: 'git version 2.47.0' });
});

test('fresh shell without a command', () => {
  expect(shellSummary(['PowerShell 7.5', '', 'PS C:\\Users\\me> '])).toBeNull();
});
