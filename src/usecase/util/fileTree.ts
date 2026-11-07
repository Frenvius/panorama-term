import type { FileChange, StatusSnapshot } from '~/domain/interfaces/git.interface';

export const STATUS_COLOR: Record<string, string> = {
  modified: '#6897bb',
  added: '#629755',
  deleted: '#6f737a',
  renamed: '#3a87ad',
  copied: '#3a87ad',
  untracked: '#d1675a',
  conflicted: '#d5756c'
};

export const statusKey = (file: FileChange): string => {
  if (file.is_untracked) return 'untracked';
  const x = file.status_index;
  const y = file.status_worktree;
  if (x === 'U' || y === 'U') return 'conflicted';
  if (x === 'A' || y === 'A') return 'added';
  if (x === 'D' || y === 'D') return 'deleted';
  if (x === 'R') return 'renamed';
  if (x === 'C') return 'copied';
  return 'modified';
};

const KEY_SEP = '\n';

export const fileKey = (repo: string, path: string): string => `${repo}${KEY_SEP}${path}`;

export const changeKey = (change: FileChange): string => fileKey(change.repo, change.path);

export const splitKey = (key: string): { repo: string; path: string } => {
  const at = key.indexOf(KEY_SEP);
  return at < 0 ? { repo: '', path: key } : { repo: key.slice(0, at), path: key.slice(at + 1) };
};

export type TreeNode =
  | { kind: 'folder'; id: string; name: string; dir: string; children: TreeNode[] }
  | { kind: 'file'; id: string; change: FileChange };

export const buildDirTree = (files: FileChange[], prefix: string): TreeNode[] => {
  const root: TreeNode[] = [];
  const folders = new Map<string, TreeNode[]>();
  for (const file of files) {
    const segments = file.dir ? file.dir.split('/').filter(Boolean) : [];
    let current = root;
    let sofar = '';
    for (const segment of segments) {
      sofar = sofar ? `${sofar}/${segment}` : segment;
      let children = folders.get(sofar);
      if (!children) {
        children = [];
        folders.set(sofar, children);
        current.push({ kind: 'folder', id: `${prefix}:${sofar}`, name: segment, dir: sofar, children });
      }
      current = children;
    }
    current.push({ kind: 'file', id: `${prefix}:${file.path}`, change: file });
  }
  return root;
};

export const sortTree = (nodes: TreeNode[]): TreeNode[] =>
  [...nodes]
    .sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === 'folder' ? -1 : 1;
      const an = a.kind === 'folder' ? a.name : a.change.name;
      const bn = b.kind === 'folder' ? b.name : b.change.name;
      return an.localeCompare(bn);
    })
    .map((node) => (node.kind === 'folder' ? { ...node, children: sortTree(node.children) } : node));

export const collectFiles = (nodes: TreeNode[]): FileChange[] =>
  nodes.flatMap((node) => (node.kind === 'file' ? [node.change] : collectFiles(node.children)));

export const collectFolderIds = (nodes: TreeNode[]): string[] =>
  nodes.flatMap((node) => (node.kind === 'folder' ? [node.id, ...collectFolderIds(node.children)] : []));

const sameChange = (a: FileChange, b: FileChange): boolean =>
  a.path === b.path &&
  a.repo === b.repo &&
  a.rename_from === b.rename_from &&
  a.is_untracked === b.is_untracked &&
  a.status_index === b.status_index &&
  a.status_worktree === b.status_worktree;

const sameChanges = (a: FileChange[], b: FileChange[]): boolean =>
  a.length === b.length && a.every((change, i) => sameChange(change, b[i]));

export const sameSnapshots = (a: Record<string, StatusSnapshot>, b: Record<string, StatusSnapshot>): boolean => {
  const keys = Object.keys(a);
  if (keys.length !== Object.keys(b).length) return false;
  return keys.every(
    (key) =>
      b[key] &&
      a[key].changes_total === b[key].changes_total &&
      a[key].unversioned_total === b[key].unversioned_total &&
      sameChanges(a[key].changes, b[key].changes) &&
      sameChanges(a[key].unversioned, b[key].unversioned)
  );
};
