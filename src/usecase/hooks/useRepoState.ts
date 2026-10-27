import React from 'react';

import type { RepoState } from '~/domain/interfaces/git.interface';
import { gitFetch, watchRepo } from '~/adapter/git/git.client';

const FETCH_MS = 180000;
const FETCH_CHECK_MS = 60000;

const fetchedAt = new Map<string, number>();
const inFlight = new Set<string>();

const fetchIfStale = (root: string) => {
  if (!document.hasFocus() || inFlight.has(root)) return;
  if (Date.now() - (fetchedAt.get(root) ?? 0) < FETCH_MS) return;
  inFlight.add(root);
  void gitFetch(root)
    .catch(() => {})
    .finally(() => {
      fetchedAt.set(root, Date.now());
      inFlight.delete(root);
    });
};

export const useRepoState = (cwd: string | undefined): RepoState | null => {
  const [repo, setRepo] = React.useState<RepoState | null>(null);

  React.useEffect(() => {
    if (!cwd) {
      setRepo(null);
      return;
    }
    return watchRepo(cwd, setRepo);
  }, [cwd]);

  const root = repo?.root;

  React.useEffect(() => {
    if (!root) return;
    const check = () => fetchIfStale(root);
    check();
    const timer = window.setInterval(check, FETCH_CHECK_MS);
    window.addEventListener('focus', check);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener('focus', check);
    };
  }, [root]);

  return repo;
};
