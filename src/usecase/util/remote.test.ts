import { describe, expect, test } from 'bun:test';
import { remoteLabel, remoteTransferLabel } from './remote';

describe('remote status labels', () => {
  test('describes each connection state and handles missing names', () => {
    expect(remoteLabel({ status: 'connected', hostName: 'Laptop', remembered: true })).toBe('Connected to Laptop');
    expect(remoteLabel({ status: 'connected', hostName: 'Laptop', remembered: true, transport: 'relay' })).toBe('Connected to Laptop · relay');
    expect(remoteLabel({ status: 'reconnecting', hostName: 'Laptop', remembered: true, error: 'Host is offline (relay)' })).toContain('Host is offline (relay)');
    expect(remoteLabel({ status: 'reconnecting', hostName: null, remembered: true })).toBe('Reconnecting to remote host...');
    expect(remoteLabel({ status: 'disconnected', hostName: 'Laptop', remembered: true })).toBe('Disconnected from Laptop');
    expect(remoteLabel({ status: 'disconnected', hostName: null, remembered: false })).toBe('Not connected');
  });
});

describe('relay transfer labels', () => {
  test('formats downloads and uploads with optional totals', () => {
    expect(remoteTransferLabel({ direction: 'down', done: 3200000, total: 12000000 })).toBe('Relay · downloading 3.2 of 12.0 MB');
    expect(remoteTransferLabel({ direction: 'up', done: 3200000, total: null })).toBe('Relay · uploading 3.2 MB');
  });
});
