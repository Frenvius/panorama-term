import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { setRemote } from '~/adapter/transport';

export let SIDECAR_HTTP = '';
export let SIDECAR_WS = '';
export let LOCAL_SIDECAR_HTTP = '';
let localEndpoint = '';
let currentEndpoint = '';

const setEndpoint = (endpoint: string): void => {
  currentEndpoint = endpoint;
  if (localEndpoint) setRemote(endpoint !== localEndpoint);
  SIDECAR_HTTP = `http://${endpoint}`;
  SIDECAR_WS = `ws://${endpoint}`;
};

export const initSidecar = async (
  getEndpoint: () => Promise<string> = () => invoke('sidecar_endpoint'),
  getLocalEndpoint: () => Promise<string> = () => invoke('local_sidecar_endpoint'),
  onEndpoint: (callback: (endpoint: string) => void) => Promise<unknown> = (callback) =>
    listen<string>('sidecar:endpoint', (event) => callback(event.payload)),
  bootstrap: () => Promise<unknown> = () => invoke('remote_bootstrap')
): Promise<void> => {
  await bootstrap();
  let changes = 0;
  await onEndpoint((endpoint) => {
    changes++;
    setEndpoint(endpoint);
  });
  const [endpoint, local] = await Promise.all([getEndpoint(), getLocalEndpoint()]);
  localEndpoint = local;
  if (changes === 0) setEndpoint(endpoint);
  else setRemote(currentEndpoint !== localEndpoint);
  LOCAL_SIDECAR_HTTP = `http://${local}`;
};

export const killPtySession = (tileId: string): Promise<unknown> =>
  fetch(`${SIDECAR_HTTP}/kill?tileId=${encodeURIComponent(tileId)}`).catch(() => undefined);

export const isTerminalBusy = async (tileId: string): Promise<boolean> => {
  try {
    const res = await fetch(`${SIDECAR_HTTP}/busy?tileId=${encodeURIComponent(tileId)}`);
    if (!res.ok) return true;
    return (await res.text()) === '1';
  } catch {
    return true;
  }
};

export const submitPtyMessage = async (tileId: string, text: string): Promise<boolean> => {
  try {
    const response = await fetch(`${SIDECAR_HTTP}/agent/send?tileId=${encodeURIComponent(tileId)}`, {
      method: 'POST',
      body: text
    });
    if (!response.ok) return false;
    const result = (await response.json()) as { ok?: boolean };
    return result.ok === true;
  } catch {
    return false;
  }
};
