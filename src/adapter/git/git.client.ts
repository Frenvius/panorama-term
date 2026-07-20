import { invoke } from '@tauri-apps/api/core';

import type {
  LogRow,
  FileDiff,
  CommitInfo,
  TrackCounts,
  BranchSnapshot,
  StatusSnapshot,
  CommitMessageEntry
} from '~/domain/interfaces/git.interface';

export const gitBranches = (path: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_branches', { path });

export const gitAheadBehind = (path: string): Promise<TrackCounts> =>
  invoke<TrackCounts>('git_ahead_behind', { path });

export const gitCheckout = (path: string, branch: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_checkout', { path, branch });

export const gitFetch = (path: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_fetch', { path });

export const gitCreateBranch = (
  path: string,
  name: string,
  checkout: boolean,
  overwrite: boolean,
  startPoint?: string
): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_create_branch', { path, name, checkout, overwrite, startPoint: startPoint ?? null });

export const gitRenameBranch = (path: string, oldName: string, newName: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_rename_branch', { path, oldName, newName });

export const gitDeleteBranch = (path: string, fullName: string, isRemote: boolean): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_delete_branch', { path, fullName, isRemote });

export const gitMergeBranch = (path: string, branch: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_merge_branch', { path, branch });

export const gitRebaseOnto = (path: string, branch: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_rebase_onto', { path, branch });

export const gitUpdateBranch = (path: string, rebase: boolean): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_update_branch', { path, rebase });

export const gitPushCurrent = (path: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_push_current', { path });

export const gitSetUpstream = (
  path: string,
  branch: string,
  upstream: string | null
): Promise<BranchSnapshot> => invoke<BranchSnapshot>('git_set_upstream', { path, branch, upstream });

export const gitCompareWithCurrent = (path: string, branch: string): Promise<CommitInfo[]> =>
  invoke<CommitInfo[]>('git_compare_with_current', { path, branch });

export const gitToggleBranchFavorite = (path: string, fullName: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_toggle_branch_favorite', { path, fullName });

export const gitStatus = (path: string): Promise<StatusSnapshot> => invoke<StatusSnapshot>('git_status', { path });

export const gitCommit = (path: string, files: string[], message: string, amend: boolean): Promise<void> =>
  invoke<void>('git_commit', { path, files, message, amend });

export const gitLogMessages = (path: string, limit = 20): Promise<CommitMessageEntry[]> =>
  invoke<CommitMessageEntry[]>('git_log_messages', { path, limit });

export const gitLogGraph = (path: string, limit = 200): Promise<LogRow[]> =>
  invoke<LogRow[]>('git_log_graph', { path, limit });

export const gitRemoteUrl = (path: string): Promise<string> => invoke<string>('git_remote_url', { path });

export const gitUnpushedCommits = (path: string): Promise<CommitMessageEntry[]> =>
  invoke<CommitMessageEntry[]>('git_unpushed_commits', { path });

export const gitDiffFile = (path: string, file: string): Promise<FileDiff> =>
  invoke<FileDiff>('git_diff_file', { path, file });

export const gitRollbackFile = (path: string, file: string): Promise<void> =>
  invoke<void>('git_rollback_file', { path, file });

export const gitAddIgnore = (path: string, pattern: string, local: boolean): Promise<void> =>
  invoke<void>('git_add_ignore', { path, pattern, local });

export const gitRevertHunk = (path: string, file: string, content: string, crlf: boolean): Promise<void> =>
  invoke<void>('git_revert_hunk', { path, file, content, crlf });

export const gitWatchFile = (path: string, file: string): Promise<number> =>
  invoke<number>('git_watch_file', { path, file });

export const gitUnwatchFile = (id: number): Promise<void> => invoke<void>('git_unwatch_file', { id });
