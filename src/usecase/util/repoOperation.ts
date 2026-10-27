import type { RepoState, RepoOperation } from '~/domain/interfaces/git.interface';

const TITLES: Record<RepoOperation, string> = {
  merging: 'Merge in progress',
  rebasing: 'Rebase in progress',
  reverting: 'Revert in progress',
  'cherry-picking': 'Cherry-pick in progress'
};

const progress = (repo: RepoState): string => (repo.step && repo.total ? ` ${repo.step}/${repo.total}` : '');

export const operationTitle = (repo: RepoState): string => (repo.state ? `${TITLES[repo.state]}${progress(repo)}` : '');

export const operationTag = (repo: RepoState): string => (repo.state ? `${repo.state}${progress(repo)}` : '');

export const canSkip = (repo: RepoState): boolean => repo.state !== null && repo.state !== 'merging';
