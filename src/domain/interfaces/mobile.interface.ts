export interface MobileDevice {
  id: string;
  name: string;
  pairedAt: number;
  desktop: boolean;
}

export type RelayStatus = 'off' | 'connecting' | 'online';

export interface MobileState {
  port: number;
  remote: boolean;
  enabled: boolean;
  relay: RelayStatus;
  hostName: string;
  listening: boolean;
  addresses: string[];
  devices: MobileDevice[];
}

export interface TileOpenRequest {
  t: 'tile.open';
  cwd: string;
  tileId: string;
  group?: { id: string; title: string };
}

export interface MobilePairing {
  uri: string;
  code: string;
  expiresAt: number;
}
