import { invoke } from '@tauri-apps/api/core';

import type { CommitInfo, BranchSnapshot } from '~/domain/interfaces/git.interface';

export const gitBranches = (path: string): Promise<BranchSnapshot> =>
  invoke<BranchSnapshot>('git_branches', { path });

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
