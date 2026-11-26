import { describe, expect, test } from 'bun:test';

import { findRollout, headStart, parseRollout, rolloutStart, rolloutDirs } from '~/usecase/util/codexFeed';

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
  const call = row({
    type: 'custom_tool_call',
    call_id: 'c1',
    name: 'exec',
    input: 'await tools.exec_command({cmd:"cargo test \\"x\\"",max_output_tokens:1})'
  });
  expect(parseRollout(call).at(-1)).toEqual({ kind: 'running', text: 'cargo test "x"' });
  const done = row({ type: 'custom_tool_call_output', call_id: 'c1', output: [] });
  expect(parseRollout(`${call}\n${done}`)).toEqual([]);
  const fn = row({
    type: 'function_call',
    call_id: 'c2',
    name: 'shell',
    arguments: JSON.stringify({ command: ['bash', '-lc', 'ls'] })
  });
  expect(parseRollout(fn)).toEqual([{ kind: 'running', text: 'ls' }]);
});

test('the end of the run carries its last message, shown once', () => {
  const raw = [row({ type: 'task_started' }), row({ type: 'task_complete', last_agent_message: 'All done.' })].join('\n');
  expect(parseRollout(raw)).toEqual([{ kind: 'done', text: 'All done.' }]);
  const answered = [
    item({ type: 'AgentMessage', content: [{ text: 'All done.' }] }),
    row({ type: 'task_complete', last_agent_message: 'All done.' })
  ].join('\n');
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

test('a log begins at the timestamp its first line opens with', () => {
  expect(headStart('{"timestamp":"2026-11-26T02:16:20.673Z","ordinal":0,"type":"session_meta","payload":{')).toBe(
    Date.UTC(2026, 10, 26, 2, 16, 20, 673)
  );
  expect(headStart('')).toBeNaN();
});

test('a run looks in the day folders around it: it can cross midnight, and the host may be a day apart', () => {
  const late = new Date(2026, 10, 24, 23, 59, 0).getTime();
  expect(rolloutDirs('/h', late, late + 120_000)).toEqual(['/h/.codex/sessions/2026/11/24', '/h/.codex/sessions/2026/11/25']);
  const early = new Date(2026, 10, 24, 0, 30, 0).getTime();
  expect(rolloutDirs('/h', early, early)).toEqual(['/h/.codex/sessions/2026/11/23', '/h/.codex/sessions/2026/11/24']);
});

describe('finding the log of a run', () => {
  const dir = '/h/.codex/sessions/2026/11/24';
  const p2 = (n: number) => String(n).padStart(2, '0');
  // Names in the host's local time, `zone` hours ahead of this client; starts in UTC.
  const logs = (zone: number, ...times: [number, number][]) => {
    const names = times.map(([h, m]) => `rollout-2026-11-24T${p2(h + zone)}-${p2(m)}-00-id${h}${m}.jsonl`);
    const starts = new Map(times.map(([h, m], i) => [`${dir}/${names[i]}`, new Date(2026, 10, 24, h, m).getTime()]));
    return {
      names,
      io: {
        list: async (d: string) => (d === dir ? names : Promise.reject(new Error('no such folder'))),
        start: async (path: string) => starts.get(path) ?? NaN
      }
    };
  };
  const at = (h: number, m: number) => new Date(2026, 10, 24, h, m).getTime();

  test('takes the first log begun after the run that no other task took', async () => {
    const { names, io } = logs(0, [10, 0], [10, 6], [10, 8]);
    expect(await findRollout('/h', at(10, 5), at(11, 0), new Set(), io)).toBe(`${dir}/${names[1]}`);
    expect(await findRollout('/h', at(10, 5), at(11, 0), new Set([`${dir}/${names[1]}`]), io)).toBe(`${dir}/${names[2]}`);
  });

  test('takes no log begun after the run ended: that one belongs to a later session', async () => {
    const { io } = logs(0, [20, 0]);
    expect(await findRollout('/h', at(10, 0), at(10, 1), new Set(), io)).toBeUndefined();
  });

  test('a host in another time zone names its logs in its own time; the start in the log still matches', async () => {
    const { names, io } = logs(3, [10, 2], [10, 9]);
    expect(await findRollout('/h', at(10, 5), at(11, 0), new Set(), io)).toBe(`${dir}/${names[1]}`);
  });
});
