import type {
  BranchLeaf,
  BranchNode,
  LocalBranch,
  RemoteBranch,
  BranchSnapshot
} from '~/domain/interfaces/git.interface';

const insertPath = (roots: BranchNode[], segments: string[], data: BranchLeaf, prefix: string): void => {
  const head = segments[0];
  const key = prefix ? `${prefix}/${head}` : head;

  if (segments.length === 1) {
    roots.push({ kind: 'branch', label: head, key, data });
    return;
  }

  let folder = roots.find((n): n is Extract<BranchNode, { kind: 'folder' }> => n.kind === 'folder' && n.label === head);
  if (!folder) {
    folder = { kind: 'folder', label: head, key, children: [] };
    roots.push(folder);
  }
  insertPath(folder.children, segments.slice(1), data, key);
};

const sortTree = (nodes: BranchNode[]): void => {
  nodes.sort((a, b) => {
    if (a.kind !== b.kind) return a.kind === 'folder' ? -1 : 1;
    return a.label.localeCompare(b.label);
  });
  for (const node of nodes) if (node.kind === 'folder') sortTree(node.children);
};

const localLeaf = (b: LocalBranch): BranchLeaf => ({
  kind: 'local',
  fullName: b.name,
  isCurrent: b.is_current,
  isFavorite: b.is_favorite,
  upstream: b.upstream ?? undefined,
  ahead: b.ahead,
  behind: b.behind
});

const remoteLeaf = (b: RemoteBranch): BranchLeaf => ({
  kind: 'remote',
  fullName: `${b.remote}/${b.branch}`,
  isCurrent: false,
  isFavorite: b.is_favorite,
  ahead: 0,
  behind: 0
});

export const buildTrees = (snap: BranchSnapshot): { local: BranchNode[]; remote: BranchNode[]; favorites: BranchNode[] } => {
  const local: BranchNode[] = [];
  const remote: BranchNode[] = [];
  const favorites: BranchNode[] = [];

  for (const b of snap.local) {
    const leaf = localLeaf(b);
    insertPath(local, b.name.split('/'), leaf, '');
    if (b.is_favorite) insertPath(favorites, b.name.split('/'), leaf, '');
  }
  for (const b of snap.remotes) {
    const leaf = remoteLeaf(b);
    const segments = [b.remote, ...b.branch.split('/')];
    insertPath(remote, segments, leaf, '');
    if (b.is_favorite) insertPath(favorites, segments, leaf, '');
  }

  sortTree(local);
  sortTree(remote);
  sortTree(favorites);
  return { local, remote, favorites };
};

export const filterTree = (nodes: BranchNode[], query: string): BranchNode[] => {
  const needle = query.trim().toLowerCase();
  if (!needle) return nodes;

  const out: BranchNode[] = [];
  for (const node of nodes) {
    if (node.kind === 'branch') {
      if (node.data.fullName.toLowerCase().includes(needle)) out.push(node);
      continue;
    }
    const children = filterTree(node.children, query);
    if (children.length > 0) out.push({ ...node, children });
  }
  return out;
};
