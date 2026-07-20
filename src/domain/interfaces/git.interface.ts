export interface LocalBranch {
  name: string;
  is_current: boolean;
  upstream: string | null;
  ahead: number;
  behind: number;
  is_favorite: boolean;
}

export interface RemoteBranch {
  remote: string;
  branch: string;
  is_favorite: boolean;
}

export interface TrackCounts {
  ahead: number;
  behind: number;
}

export interface BranchSnapshot {
  current: string | null;
  local: LocalBranch[];
  remotes: RemoteBranch[];
  recent: string[];
}

export interface CommitInfo {
  hash: string;
  short: string;
  subject: string;
  author: string;
  date: string;
}

export interface FileChange {
  path: string;
  name: string;
  dir: string;
  status_index: string;
  status_worktree: string;
  is_untracked: boolean;
  rename_from: string | null;
}

export interface StatusSnapshot {
  changes: FileChange[];
  unversioned: FileChange[];
}

export interface FileDiff {
  old: string;
  new: string;
  binary: boolean;
  crlf: boolean;
}

export interface LogRow {
  short: string;
  parents: string[];
  author: string;
  committer: string;
  date: string;
  refs: string;
  message: string;
}

export interface CommitMessageEntry {
  short: string;
  subject: string;
  body: string;
  date: string;
}

export type BranchKind = 'local' | 'remote';

export interface BranchLeaf {
  kind: BranchKind;
  fullName: string;
  isCurrent: boolean;
  isFavorite: boolean;
  upstream?: string;
  ahead: number;
  behind: number;
}

export type BranchNode =
  | { kind: 'folder'; label: string; key: string; children: BranchNode[] }
  | { kind: 'branch'; label: string; key: string; data: BranchLeaf };

export type BranchAction =
  | { type: 'checkout'; branch: string }
  | { type: 'new-from'; branch: string }
  | { type: 'update' }
  | { type: 'push' }
  | { type: 'compare'; branch: string }
  | { type: 'merge'; branch: string }
  | { type: 'rebase'; branch: string }
  | { type: 'rename'; branch: string }
  | { type: 'delete'; branch: string; isRemote: boolean }
  | { type: 'set-upstream'; branch: string; upstream: string | null }
  | { type: 'toggle-favorite'; branch: string };
