import { expect, test } from 'bun:test';

import { parseRollout, pickRollout, rolloutStart, rolloutDirs } from '~/usecase/util/codexFeed';

const row = (payload: Record<string, unknown>) => JSON.stringify({ timestamp: 't', type: 'event_msg', payload });
const item = (value: Record<string, unknown>) => row({ type: 'item_completed', item: value });

test('completed items become feed lines in order', () => {
  const raw = [
    item({ type: 'AgentMessage', content: [{ type: 'Text', text: 'Reading the spec.' }] }),
    item({ type: 'CommandExecution', command: ['/bin/bash', '-lc', 'bun test'], exit_code: 1 }),
    item({ type: 'McpToolCall', server: 'pixellab', tool: 'inpaint', status: 'completed' }),
    item({ type: 'FileChange', changes: { 'src/a.ts': {}, 'src/b.ts': {} } }),
    item({ type: 'Reasoning', summary_text: ['Checking the tests'] }),
    item({ type: 'Reasoning', summary_text: [] }),
    item({ type: 'UserMessage', content: [{ text: 'the prompt' }] })
  ].join('\n');
  expect(parseRollout(raw)).toEqual([
    { kind: 'message', text: 'Reading the spec.' },
    { kind: 'command', text: 'bun test', failed: true },
    { kind: 'tool', text: 'pixellab.inpaint', failed: false },
    { kind: 'edit', text: 'src/a.ts, src/b.ts' },
    { kind: 'reasoning', text: 'Checking the tests' }
  ]);
});

test('a call without output yet shows as running', () => {
  const call = row({ type: 'custom_tool_call', call_id: 'c1', name: 'exec', input: 'await tools.exec_command({cmd:"cargo test \\"x\\"",max_output_tokens:1})' });
  expect(parseRollout(call).at(-1)).toEqual({ kind: 'running', text: 'cargo test "x"' });
  const done = row({ type: 'custom_tool_call_output', call_id: 'c1', output: [] });
  expect(parseRollout(`${call}\n${done}`)).toEqual([]);
  const fn = row({ type: 'function_call', call_id: 'c2', name: 'shell', arguments: JSON.stringify({ command: ['bash', '-lc', 'ls'] }) });
  expect(parseRollout(fn)).toEqual([{ kind: 'running', text: 'ls' }]);
});

test('the end of the run carries its last message, shown once', () => {
  const raw = [row({ type: 'task_started' }), row({ type: 'task_complete', last_agent_message: 'All done.' })].join('\n');
  expect(parseRollout(raw)).toEqual([{ kind: 'done', text: 'All done.' }]);
  const answered = [item({ type: 'AgentMessage', content: [{ text: 'All done.' }] }), row({ type: 'task_complete', last_agent_message: 'All done.' })].join('\n');
  expect(parseRollout(answered)).toEqual([{ kind: 'done', text: 'All done.' }]);
});

test('a multi-line script shows its first line', () => {
  const raw = item({ type: 'CommandExecution', command: ['/bin/bash', '-lc', "python3 - <<'PY'\nprint(1)\nPY"], exit_code: 0 });
  expect(parseRollout(raw)).toEqual([{ kind: 'command', text: "python3 - <<'PY' …", failed: false }]);
});

test('a tail cut mid-line and broken rows are skipped', () => {
  const raw = `put":"half a line"}\n{not json\n${item({ type: 'AgentMessage', content: [{ text: 'ok' }] })}\n`;
  expect(parseRollout(raw)).toEqual([{ kind: 'message', text: 'ok' }]);
});

test('rollout names give their local start time', () => {
  expect(rolloutStart('rollout-2026-11-24T22-22-53-01a2104c-6ad5.jsonl')).toBe(new Date(2026, 10, 24, 22, 22, 53).getTime());
  expect(rolloutStart('notes.txt')).toBeNaN();
});

test('a run takes the first log begun after it that is still free', () => {
  const p2 = (n: number) => String(n).padStart(2, '0');
  const at = (h: number, m: number, s: number) => `rollout-2026-11-24T${p2(h)}-${p2(m)}-${p2(s)}-id${h}${m}${s}.jsonl`;
  const names = [at(10, 0, 0), at(10, 5, 2), at(10, 5, 30), 'other.jsonl'];
  const started = new Date(2026, 10, 24, 10, 5, 0).getTime();
  expect(pickRollout(names, started, new Set())).toBe(at(10, 5, 2));
  expect(pickRollout(names, started, new Set([at(10, 5, 2)]))).toBe(at(10, 5, 30));
  expect(pickRollout(names, started + 3600_000, new Set())).toBeUndefined();
});

test('a run near midnight looks in both day folders', () => {
  const start = new Date(2026, 10, 24, 23, 59, 0).getTime();
  expect(rolloutDirs('/h', start, start + 120_000)).toEqual(['/h/.codex/sessions/2026/11/24', '/h/.codex/sessions/2026/11/25']);
  expect(rolloutDirs('/h', start, start)).toEqual(['/h/.codex/sessions/2026/11/24']);
});
