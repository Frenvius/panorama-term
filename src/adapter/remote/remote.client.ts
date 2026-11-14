import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface RemoteStatus {
  status: 'connected' | 'reconnecting' | 'disconnected';
  hostName: string | null;
  remembered: boolean;
  transport?: 'lan' | 'relay';
  error?: string | null;
}

export const remoteStatus = (): Promise<RemoteStatus> => invoke('remote_status');
export const onRemoteStatus = (callback: (status: RemoteStatus) => void) =>
  listen<RemoteStatus>('remote:status', (event) => callback(event.payload));
export const pairRemote = (uri: string, name: string): Promise<string> =>
  invoke('remote_pair', { uri: uri.trim(), name: name.trim() || 'Desktop' });
export const connectRemote = (): Promise<string> => invoke('remote_connect');
export const disconnectRemote = (): Promise<void> => invoke('remote_disconnect');
export const forgetRemote = (): Promise<void> => invoke('remote_forget');
