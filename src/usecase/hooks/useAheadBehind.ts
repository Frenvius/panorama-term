import React from 'react';

import type { TrackCounts } from '~/domain/interfaces/git.interface';
import { gitFetch, gitAheadBehind } from '~/adapter/git/git.client';

const POLL_MS = 15000;
const FETCH_MS = 180000;
const NONE: TrackCounts = { ahead: 0, behind: 0 };

const fetchedAt = new Map<string, number>();
const inFlight = new Map<string, Promise<unknown>>();

const maybeFetch = (cwd: string): Promise<unknown> => {
  const pending = inFlight.get(cwd);
  if (pending) return pending;
  if (Date.now() - (fetchedAt.get(cwd) ?? 0) < FETCH_MS) return Promise.resolve();

  const run = gitFetch(cwd)
    .catch(() => {})
    .finally(() => {
      fetchedAt.set(cwd, Date.now());
      inFlight.delete(cwd);
    });
  inFlight.set(cwd, run);
  return run;
};

export const useAheadBehind = (cwd: string | undefined, branch: string | undefined): TrackCounts => {
  const [track, setTrack] = React.useState<TrackCounts>(NONE);

  React.useEffect(() => {
    if (!cwd || !branch) {
      setTrack(NONE);
      return;
    }
    let alive = true;
    const refresh = () => {
      void maybeFetch(cwd).then(() => {
        if (!alive) return;
        gitAheadBehind(cwd)
          .then((t) => alive && setTrack(t))
          .catch(() => alive && setTrack(NONE));
      });
    };
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [cwd, branch]);

  return track;
};
