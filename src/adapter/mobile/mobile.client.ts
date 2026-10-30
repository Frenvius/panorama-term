import type { MobileState, MobilePairing } from '~/domain/interfaces/mobile.interface';

import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';

const request = async <T>(path: string, method = 'GET'): Promise<T> => {
  const response = await fetch(`${SIDECAR_HTTP}/mobile/${path}`, { method });
  if (!response.ok) throw new Error(`mobile ${path} failed: ${response.status}`);
  return (await response.json()) as T;
};

export const fetchMobileState = (): Promise<MobileState> => request<MobileState>('state');

export const startMobilePairing = (): Promise<MobilePairing> => request<MobilePairing>('pair', 'POST');

export const cancelMobilePairing = (): Promise<MobileState> => request<MobileState>('cancel', 'POST');

export const setMobileEnabled = (on: boolean): Promise<MobileState> =>
  request<MobileState>(`enable?on=${on ? 1 : 0}`, 'POST');

export const setMobileRemote = (on: boolean): Promise<MobileState> =>
  request<MobileState>(`remote?on=${on ? 1 : 0}`, 'POST');

export const revokeMobileDevice = (id: string): Promise<MobileState> =>
  request<MobileState>(`revoke?id=${encodeURIComponent(id)}`, 'POST');
