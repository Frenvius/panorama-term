import type { FileChange } from '~/domain/interfaces/git.interface';

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

export type TreeNode =
  { kind: 'folder'; id: string; name: string; children: TreeNode[] } | { kind: 'file'; id: string; change: FileChange };

export const buildDirTree = (files: FileChange[], prefix: string): TreeNode[] => {
  const root: TreeNode[] = [];
  for (const file of files) {
    const segments = file.dir ? file.dir.split('/').filter(Boolean) : [];
    let current = root;
    let sofar = '';
    for (const segment of segments) {
      sofar = sofar ? `${sofar}/${segment}` : segment;
      let folder = current.find((n): n is Extract<TreeNode, { kind: 'folder' }> => n.kind === 'folder' && n.name === segment);
      if (!folder) {
        folder = { kind: 'folder', id: `${prefix}:${sofar}`, name: segment, children: [] };
        current.push(folder);
      }
      current = folder.children;
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

export const collectPaths = (nodes: TreeNode[]): string[] =>
  nodes.flatMap((node) => (node.kind === 'file' ? [node.change.path] : collectPaths(node.children)));

export const collectFiles = (nodes: TreeNode[]): FileChange[] =>
  nodes.flatMap((node) => (node.kind === 'file' ? [node.change] : collectFiles(node.children)));

export const collectFolderIds = (nodes: TreeNode[]): string[] =>
  nodes.flatMap((node) => (node.kind === 'folder' ? [node.id, ...collectFolderIds(node.children)] : []));

export const flattenTree = (nodes: TreeNode[], shut: (id: string) => boolean): string[] =>
  nodes.flatMap((node) => {
    if (node.kind === 'file') return [node.change.path];
    return shut(node.id) ? [] : flattenTree(node.children, shut);
  });
