import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';

export interface AppUpdateStatus {
  status: string;
  error: string | null;
  version: string | null;
  downloaded: number;
  contentLength: number;
  currentVersion: string | null;
}

export const postUpdateStatus = (state: AppUpdateStatus): void => {
  if (!SIDECAR_HTTP) return;
  const body = JSON.stringify({
    status: state.status,
    version: state.version,
    currentVersion: state.currentVersion,
    error: state.error,
    downloaded: state.downloaded,
    contentLength: state.contentLength
  });
  fetch(`${SIDECAR_HTTP}/app/update-status`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body
  }).catch(() => undefined);
};
