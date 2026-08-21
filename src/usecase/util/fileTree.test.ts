import { expect, test } from 'bun:test';

import type { FileChange } from '~/domain/interfaces/git.interface';
import { sortTree, buildDirTree } from '~/usecase/util/fileTree';

const change = (path: string): FileChange => {
  const slash = path.lastIndexOf('/');
  return {
    path,
    name: slash < 0 ? path : path.slice(slash + 1),
    dir: slash < 0 ? '' : path.slice(0, slash),
    status_index: 'M',
    status_worktree: ' ',
    is_untracked: false,
    rename_from: null
  };
};

const labels = (nodes: ReturnType<typeof buildDirTree>): string[] =>
  nodes.map((node) => (node.kind === 'folder' ? `${node.name}/` : node.change.name));

test('sortTree puts folders before files and sorts each alphabetically', () => {
  const files = ['README.md', 'src/b.ts', 'build.py', 'docs/a.md', 'src/a.ts'].map(change);
  const tree = sortTree(buildDirTree(files, 'x'));

  expect(labels(tree)).toEqual(['docs/', 'src/', 'build.py', 'README.md']);

  const src = tree.find((node) => node.kind === 'folder' && node.name === 'src');
  expect(src?.kind === 'folder' && labels(src.children)).toEqual(['a.ts', 'b.ts']);
});
