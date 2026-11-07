import { expect, test } from 'bun:test';

import type { FileChange } from '~/domain/interfaces/git.interface';
import { sortTree, buildDirTree, collectFiles, sameSnapshots } from '~/usecase/util/fileTree';

const change = (path: string): FileChange => {
  const slash = path.lastIndexOf('/');
  return {
    repo: '/repo',
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

test('buildDirTree keeps every file of a very wide folder in input order', () => {
  const files = Array.from({ length: 50000 }, (_, i) => change(`data/fuzz/${i}.bin`));
  const tree = buildDirTree(files, 'x');

  expect(labels(tree)).toEqual(['data/']);
  expect(collectFiles(tree).map((file) => file.path)).toEqual(files.map((file) => file.path));
});

test('sameSnapshots detects status changes and ignores identity', () => {
  const snap = () => ({
    '/repo': { changes: [change('a.ts')], changes_total: 1, unversioned: [], unversioned_total: 9000 }
  });
  const moved = snap();
  moved['/repo'].changes[0].status_worktree = 'D';
  const grown = snap();
  grown['/repo'].unversioned_total = 9001;

  expect(sameSnapshots(snap(), snap())).toBe(true);
  expect(sameSnapshots(snap(), moved)).toBe(false);
  expect(sameSnapshots(snap(), grown)).toBe(false);
  expect(sameSnapshots(snap(), {})).toBe(false);
});
