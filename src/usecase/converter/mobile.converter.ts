import type { MobileState } from '~/domain/interfaces/mobile.interface';

export const relayStatusLabel = (state: MobileState): string => {
  if (!state.remote) return 'Off.';
  if (state.devices.length === 0) return 'Starts after the first phone is paired.';
  if (state.relay === 'online') return 'Online.';
  return 'Connecting...';
};
