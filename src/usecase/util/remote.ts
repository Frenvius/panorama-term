import type { RemoteStatus } from '~/adapter/remote/remote.client';

export const remoteLabel = (status: RemoteStatus): string => {
  if (status.status === 'reconnecting') return `Reconnecting to ${status.hostName || 'remote host'}...${status.error ? ` ${status.error}` : ''}`;
  if (status.status === 'connected') return `Connected to ${status.hostName || 'remote host'}${status.transport === 'relay' ? ' · relay' : ''}`;
  return status.remembered ? `Disconnected from ${status.hostName || 'remote host'}` : 'Not connected';
};

export interface RemoteTransfer {
  direction: 'up' | 'down';
  done: number;
  total: number | null;
}

export const remoteTransferLabel = (transfer: RemoteTransfer): string => {
  const amount = (bytes: number) => (bytes / 1_000_000).toFixed(1);
  const verb = transfer.direction === 'down' ? 'downloading' : 'uploading';
  return `Relay · ${verb} ${amount(transfer.done)}${transfer.total == null ? '' : ` of ${amount(transfer.total)}`} MB`;
};
