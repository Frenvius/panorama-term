import { describe, expect, test } from 'bun:test';
import { createTransport, hostCommands } from './transport';

describe('transport', () => {
  test('local calls invoke directly, remote routes host commands and keeps local commands local', async () => {
    const calls: unknown[][] = [];
    const direct = Promise.resolve('result');
    const invoke = ((...args: unknown[]) => {
      calls.push(args);
      return direct;
    }) as typeof import('@tauri-apps/api/core').invoke;
    let remote = false;
    const call = createTransport(invoke, () => remote);
    expect(call('git_status', { path: '/repo' })).toBe(direct);
    remote = true;
    await call('git_status', { path: '/repo' });
    for (const command of ['plugins_scan', 'log_ui_error', 'open_url', 'has_clipboard_image', 'updater']) await call(command);
    await call('store_read', { name: 'config.json' });
    await call('store_read', { name: 'ui-state.json' });
    await call('store_read', { name: 'workspaces.json' });
    expect(calls[0]).toEqual(['git_status', { path: '/repo' }]);
    expect(calls[1]).toEqual(['remote_invoke', { command: 'git_status', args: { path: '/repo' } }]);
    expect(calls.slice(2, 9).map((c) => c[0])).toEqual([
      'plugins_scan',
      'log_ui_error',
      'open_url',
      'has_clipboard_image',
      'updater',
      'store_read',
      'store_read'
    ]);
    expect(calls[9]).toEqual(['remote_invoke', { command: 'store_read', args: { name: 'workspaces.json' } }]);
  });
  test('remote reads of the tasks tile go to the host: output tails and the home Codex logs live under', async () => {
    const calls: unknown[][] = [];
    const invoke = ((...args: unknown[]) => {
      calls.push(args);
      return Promise.resolve('');
    }) as typeof import('@tauri-apps/api/core').invoke;
    const call = createTransport(invoke, () => true);
    await call('read_file_tail', { path: '/tmp/a.output', maxBytes: 10 });
    await call('home_dir');
    expect(calls.map((c) => c[0])).toEqual(['remote_invoke', 'remote_invoke']);
  });
  test('returns raw remote bytes and splits mixed store writes', async () => {
    const calls: unknown[][] = [];
    const invoke = ((...args: unknown[]) => {
      calls.push(args);
      return Promise.resolve(new Uint8Array([0, 255]).buffer);
    }) as typeof import('@tauri-apps/api/core').invoke;
    const call = createTransport(invoke, () => true);
    expect([...new Uint8Array(await call<ArrayBuffer>('read_file_bytes', { path: '/file' }))]).toEqual([0, 255]);
    expect(calls[0]).toEqual(['remote_invoke_bytes', { command: 'read_file_bytes', args: { path: '/file' } }]);
    await call('store_write_many', {
      entries: [
        { name: 'config.json', value: 1 },
        { name: 'workspaces/a', value: 2 }
      ]
    });
    expect(calls[1]).toEqual(['store_write_many', { entries: [{ name: 'config.json', value: 1 }] }]);
    expect(calls[2]).toEqual([
      'remote_invoke',
      { command: 'store_write_many', args: { entries: [{ name: 'workspaces/a', value: 2 }] } }
    ]);
  });
});

test('host command list matches the Rust dispatcher', async () => {
  const source = await Bun.file('src-tauri/src/service_bridge.rs').text();
  const dispatch = source.slice(source.indexOf('pub async fn dispatch('), source.indexOf('async fn cleanup('));
  const rust = new Set([...dispatch.matchAll(/"([a-z][a-z0-9_]+)"\s*(?:\||=>)/g)].map((m) => m[1]!));
  for (const command of ['store_read', 'store_write', 'store_write_many', 'store_delete', 'store_list', 'read_bytes_chunk'])
    rust.delete(command);
  expect([...hostCommands].sort()).toEqual([...rust].sort());
});
