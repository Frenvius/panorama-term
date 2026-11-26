import { describe, expect, test } from 'bun:test';

import { register } from './register';

type Handler = (...args: any[]) => any;
const handlers = new Map<string, Handler>();
register(((event: string, filter: unknown, handler?: Handler) => {
  const key = typeof filter === 'function' ? event : `${event}:${(filter as { component: string }).component}`;
  handlers.set(key, (typeof filter === 'function' ? filter : handler) as Handler);
}) as any);

const posts: Record<string, any>[] = [];
let agents: () => Promise<unknown[]> = async () => [];
const $ = {
  env: { get: async (key: string) => (key === 'PANORAMA_TILE_ID' ? 't1' : undefined) },
  http: { fetch: async (_url: string, init: { body: string }) => void posts.push(JSON.parse(init.body)) },
  session: { id: async () => 's1' },
  agent: { list: () => agents() },
  ui: { log: () => {} }
};
const last = (key: string) => posts.filter((p) => key in p).at(-1)?.[key];
// The row with `id` in the last report of `key`; the tests share the mod's state, so they look rows up by id.
const row = (key: string, id: string) => (last(key) ?? []).find((r: { id: string }) => r.id === id);
const toolCall = (input: Record<string, unknown>, next: () => Promise<unknown>) => handlers.get('tool.call')!($, input, next);
// The render handler posts without waiting; a tick lets the post land.
const taskMessage = async (id: string, status?: string) => {
  handlers.get('ui.render:UserMessage')!($, { props: { task: { id, status } } }, () => null);
  await Bun.sleep(1);
};

describe('background tasks in the Claude mod', () => {
  // First: no shell has named its output folder yet.
  test('a monitor started before any shell gets its output once a shell reveals the folder', async () => {
    await toolCall({ tool: 'Monitor', tool_use_id: 'c4', command: 'tail -f log' }, async () => ({ result: { taskId: 'm4' } }));
    expect(row('shells', 'm4').outputFile).toBeUndefined();
    await toolCall({ tool: 'Bash', tool_use_id: 'c5', command: 'make' }, async () => ({
      result: { backgroundTaskId: 'b5' },
      text: 'Output is being written to: /tmp/s/tasks/b5.output'
    }));
    expect(row('shells', 'm4').outputFile).toBe('/tmp/s/tasks/m4.output');
  });

  test('a call that throws ends as killed instead of staying listed', async () => {
    const call = { tool: 'Bash', tool_use_id: 'c1', command: 'codex exec "fix it"' };
    await expect(toolCall(call, () => Promise.reject(new Error('interrupted')))).rejects.toThrow('interrupted');
    expect(row('shells', 'c1')).toBeUndefined();
    expect(row('endedShells', 'c1').status).toBe('killed');
  });

  test('a call moved to the background keeps one row, which ends with the task', async () => {
    const call = { tool: 'Bash', tool_use_id: 'c2', command: 'codex exec "review"' };
    await toolCall(call, async () => ({
      result: { backgroundTaskId: 'b2' },
      text: 'Output is being written to: /tmp/t/b2.output'
    }));
    expect(row('shells', 'c2')).toEqual(
      expect.objectContaining({ command: 'codex exec "review"', outputFile: '/tmp/t/b2.output' })
    );
    expect(row('endedShells', 'c2')).toBeUndefined();
    await taskMessage('b2', 'failed');
    expect(row('shells', 'c2')).toBeUndefined();
    expect(row('endedShells', 'c2').status).toBe('failed');
  });

  test('an ended command is clipped, so many of them stay under the post limit', async () => {
    await toolCall({ tool: 'Bash', tool_use_id: 'c9', command: `codex exec "${'x'.repeat(5000)}"` }, async () => ({}));
    expect(row('endedShells', 'c9').command.length).toBe(1000);
  });

  test('a report without a status does not overwrite how a task ended', async () => {
    await toolCall({ tool: 'Bash', tool_use_id: 'c3', command: 'sleep 9' }, async () => ({ result: { backgroundTaskId: 'b3' } }));
    await taskMessage('b3', 'killed');
    await taskMessage('b3');
    expect(row('endedShells', 'b3').status).toBe('killed');
  });

  test('only an agent CLI in command position is tracked', async () => {
    const tracked = async (command: string, id: string) => {
      await toolCall({ tool: 'Bash', tool_use_id: id, command }, async () => ({}));
      return Boolean(row('endedShells', id));
    };
    expect(await tracked('grep -R codex .', 'g1')).toBe(false);
    expect(await tracked('git log | grep gemini', 'g2')).toBe(false);
    expect(await tracked('cd app && codex exec "x"', 'g3')).toBe(true);
    expect(await tracked('& "C:\\Program Files\\nodejs\\codex.cmd" exec x', 'g4')).toBe(true);
    expect(await tracked('FOO=1 ~/.local/bin/codex exec x', 'g5')).toBe(true);
  });

  test('a failed subagent list ends nothing; a later one that misses the agent ends it with its row', async () => {
    const running = { id: 'a1', type: 'Explore', description: 'map', status: 'running' };
    agents = async () => [running];
    await toolCall({ tool: 'Read', tool_use_id: 'r1', agentId: 'a1', file_path: '/x' }, async () => ({}));
    await Bun.sleep(0);
    agents = () => Promise.reject(new Error('busy'));
    await toolCall({ tool: 'Read', tool_use_id: 'r2', agentId: 'a1', file_path: '/y' }, async () => ({}));
    await Bun.sleep(0);
    expect(last('endedAgents') ?? []).toEqual([]);
    agents = async () => [{ ...running, status: 'completed', taskStatus: 'failed' }];
    await toolCall({ tool: 'Read', tool_use_id: 'r3', agentId: 'other', file_path: '/z' }, async () => ({}));
    await Bun.sleep(0);
    expect(row('subagents', 'a1')).toBeUndefined();
    expect(row('endedAgents', 'a1')).toEqual(expect.objectContaining({ type: 'Explore', status: 'failed' }));
  });

  test('a task stopped with TaskStop ends as killed, and the Stop hook keeps it so', async () => {
    await toolCall({ tool: 'Monitor', tool_use_id: 'c6', command: 'tail -F log' }, async () => ({ result: { taskId: 'm6' } }));
    await toolCall({ tool: 'TaskStop', tool_use_id: 'c7', task_id: 'm6' }, async () => ({}));
    expect(row('shells', 'm6')).toBeUndefined();
    expect(row('endedShells', 'm6').status).toBe('killed');
    await handlers.get('classic.Stop')!($, { background_tasks: [] }, async () => ({}));
    expect(row('endedShells', 'm6').status).toBe('killed');
  });

  test('a subagent that handed back ends though it waits to be resumed', async () => {
    agents = async () => [{ id: 'a2', type: 'general-purpose', description: 'count', status: 'running', taskStatus: 'running' }];
    await toolCall({ tool: 'Bash', tool_use_id: 'r4', agentId: 'a2', command: 'ls' }, async () => ({}));
    await Bun.sleep(0);
    expect(row('subagents', 'a2')).toBeDefined();
    agents = async () => [
      { id: 'a2', type: 'general-purpose', description: 'count', status: 'waiting', taskStatus: 'completed' }
    ];
    await toolCall({ tool: 'Read', tool_use_id: 'r5', agentId: 'other', file_path: '/z' }, async () => ({}));
    await Bun.sleep(0);
    expect(row('subagents', 'a2')).toBeUndefined();
    expect(row('endedAgents', 'a2').status).toBe('completed');
  });
});
