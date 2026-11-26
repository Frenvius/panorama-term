import { invoke as tauriInvoke } from '@tauri-apps/api/core';

let remote = false;
export const setRemote = (value: boolean): void => {
  remote = value;
};
export const isRemote = (): boolean => remote;

export const hostCommands = new Set([
  'read_dir',
  'read_text_file',
  'read_file_head',
  'read_file_tail',
  'home_dir',
  'read_file_bytes',
  'write_text_file',
  'create_entry',
  'watch_text_file',
  'unwatch_text_file',
  'git_root',
  'git_branches',
  'git_watch_repo',
  'git_unwatch_repo',
  'git_checkout',
  'git_operation',
  'git_fetch',
  'git_create_branch',
  'git_rename_branch',
  'git_delete_branch',
  'git_resolve_ref',
  'git_merge_branch',
  'git_rebase_onto',
  'git_update_branch',
  'git_push_current',
  'git_set_upstream',
  'git_compare_with_current',
  'git_toggle_branch_favorite',
  'git_repos',
  'git_blob',
  'git_status',
  'git_commit',
  'git_log_messages',
  'git_log_graph',
  'git_remote_url',
  'git_unpushed_commits',
  'git_diff_file',
  'git_commit_files',
  'git_diff_with_ref',
  'git_commit_diff_file',
  'git_rollback_file',
  'git_add_ignore',
  'git_revert_hunk',
  'git_watch_file',
  'git_unwatch_file',
  'read_note',
  'write_note',
  'delete_note',
  'link_note',
  'unlink_note',
  'link_term',
  'unlink_term',
  'pi_session_summary',
  'claude_session_summary',
  'agent_history',
  'agent_usage',
  'run_watch_manifests',
  'run_unwatch_manifests',
  'docker_available',
  'docker_ps',
  'docker_action',
  'docker_engine',
  'upload_image_chunk',
  'read_temp_image'
]);
const hostStore = (name: unknown): boolean =>
  typeof name === 'string' &&
  (name === 'workspaces.json' || name === 'workspaces' || name.startsWith('workspaces/') || name === 'run-defaults');
const bytesCommands = new Set(['read_file_bytes', 'git_blob', 'read_temp_image']);

type Invoke = typeof tauriInvoke;
export const createTransport = (invoke: Invoke, remoteMode: () => boolean) => {
  const call = <T>(command: string, args: Record<string, unknown> = {}): Promise<T> => {
    if (!remoteMode()) return invoke<T>(command, args);
    if (command === 'store_write_many') {
      const entries = args.entries as { name: string; value: unknown }[];
      const local = entries.filter((entry) => !hostStore(entry.name));
      const host = entries.filter((entry) => hostStore(entry.name));
      if (!host.length) return invoke<T>(command, args);
      if (!local.length) return invoke<T>('remote_invoke', { command, args });
      return Promise.all([
        invoke(command, { entries: local, ...(args.origin ? { origin: args.origin } : {}) }),
        invoke('remote_invoke', { command, args: { entries: host, ...(args.origin ? { origin: args.origin } : {}) } })
      ]).then(() => undefined as T);
    }
    const toHost = hostCommands.has(command) || (command.startsWith('store_') && hostStore(args.name ?? args.dir));
    if (!toHost) return invoke<T>(command, args);
    return invoke<T>(bytesCommands.has(command) ? 'remote_invoke_bytes' : 'remote_invoke', { command, args });
  };
  return call;
};
export const invoke = createTransport(tauriInvoke, isRemote);
