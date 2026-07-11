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
