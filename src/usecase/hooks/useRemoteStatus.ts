import React from 'react';
import { onRemoteStatus, remoteStatus, type RemoteStatus } from '~/adapter/remote/remote.client';

export const useRemoteStatus = () => {
  const [status, setStatus] = React.useState<RemoteStatus | null>(null);
  React.useEffect(() => {
    let active = true;
    let changed = false;
    const subscription = onRemoteStatus((next) => {
      changed = true;
      if (active) setStatus(next);
    });
    void subscription
      .then(() => remoteStatus())
      .then((next) => {
        if (active && !changed) setStatus(next);
      })
      .catch(() => {});
    return () => {
      active = false;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);
  return status;
};
