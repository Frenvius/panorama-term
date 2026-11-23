import type { CandidateKey, CandidateEntry, MetricsRow } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { getMetrics, listCandidates, clearCandidate } from '~/adapter/orch/orch.client';

export const useOrchModelStatus = () => {
  const [candidates, setCandidates] = React.useState<CandidateEntry[]>([]);
  const [metrics, setMetrics] = React.useState<MetricsRow[]>([]);
  const [error, setError] = React.useState<string | null>(null);

  const refresh = React.useCallback(() => {
    Promise.all([listCandidates(), getMetrics()])
      .then(([c, m]) => {
        setCandidates(c.candidates);
        setMetrics(m.rows);
        setError(null);
      })
      .catch((err) => setError(err instanceof Error ? err.message : String(err)));
  }, []);

  React.useEffect(() => {
    refresh();
  }, [refresh]);

  const clear = React.useCallback(
    (key: CandidateKey) => {
      clearCandidate(key)
        .then(refresh)
        .catch((err) => setError(err instanceof Error ? err.message : String(err)));
    },
    [refresh]
  );

  return { candidates, metrics, error, clear };
};
