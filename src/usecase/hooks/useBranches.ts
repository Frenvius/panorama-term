import React from 'react';

import type { BranchSnapshot } from '~/domain/interfaces/git.interface';
import { gitBranches } from '~/adapter/git/git.client';

export const useBranches = (cwd: string | undefined, open: boolean) => {
  const [snapshot, setSnapshot] = React.useState<BranchSnapshot | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!open || !cwd) return;
    let alive = true;
    setLoading(true);
    gitBranches(cwd)
      .then((snap) => alive && setSnapshot(snap))
      .catch((err: unknown) => alive && setError(typeof err === 'string' ? err : String(err)))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, [open, cwd]);

  return { snapshot, loading, error, setSnapshot, setError };
};
