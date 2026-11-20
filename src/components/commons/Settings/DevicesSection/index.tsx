import React from 'react';

import QrCode from '~/components/commons/QrCode';
import { useMobileAccess } from '~/usecase/hooks/useMobileAccess';
import { relayStatusLabel } from '~/usecase/converter/mobile.converter';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Switch from '~/components/commons/Settings/Switch';
import Button from '~/components/commons/Settings/Button';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';

import styles from './styles.module.scss';

const QR_SIZE = 192;

const minutesLeft = (expiresAt: number): number => Math.max(1, Math.ceil((expiresAt - Date.now()) / 60000));

const DevicesSection = () => {
  const { state, pairing, error, toggleEnabled, toggleRemote, pair, cancelPairing, revoke } = useMobileAccess();
  const [desktop, setDesktop] = React.useState(false);
  const [revokeTarget, setRevokeTarget] = React.useState<string | null>(null);

  const pairPhone = () => {
    setDesktop(false);
    pair(false);
  };

  const pairDesktop = () => {
    setDesktop(true);
    pair(true);
  };

  const confirmRevoke = () => {
    if (revokeTarget) revoke(revokeTarget);
    setRevokeTarget(null);
  };

  const cancelRevoke = () => setRevokeTarget(null);

  if (!state) {
    return (
      <Group label="Connections">
        <div className={styles.loadingRow}>Loading...</div>
      </Group>
    );
  }

  const revokeDevice = state.devices.find((d) => d.id === revokeTarget);

  return (
    <>
      {revokeTarget && (
        <ConfirmDialog
          title="Remove device"
          message={`Remove "${revokeDevice?.name ?? revokeTarget}"? It will need to be paired again to reconnect.`}
          confirmLabel="Remove"
          danger
          onConfirm={confirmRevoke}
          onCancel={cancelRevoke}
        />
      )}
      <Group label="Connections">
        {error && <p className={styles.portError}>{error}</p>}
        <Row
          label="Allow device connections"
          description={`Listen on port ${state.port} for paired devices on this network. Traffic is end-to-end encrypted.`}
        >
          <Switch checked={state.enabled} onChange={toggleEnabled} label="Allow device connections" />
        </Row>
        {state.enabled && (
          <Row
            label="Allow access away from home"
            description={`Paired phones reach this computer through the Panorama relay from any network. ${relayStatusLabel(state)}`}
          >
            <Switch checked={state.remote} onChange={toggleRemote} label="Allow access away from home" />
          </Row>
        )}
        {state.enabled && !state.listening && (
          <p className={styles.portError}>Port {state.port} could not be opened. Another app may be using it.</p>
        )}
      </Group>
      {state.enabled && (
        <Group label="Pair a device">
          {pairing ? (
            <div className={styles.pairingCard}>
              <QrCode size={QR_SIZE} value={pairing.uri} />
              <div className={styles.pairingInfo}>
                <p className={styles.pairingHint}>
                  {desktop
                    ? 'Copy this URI into Remote host settings on the other desktop. It grants full access to this computer.'
                    : 'Scan with the Panorama app, or type the code.'}
                </p>
                <span className={styles.pairingCode}>{pairing.code}</span>
                {desktop && (
                  <textarea
                    className={styles.uriArea}
                    readOnly
                    value={pairing.uri}
                    aria-label="Desktop pairing URI"
                    onFocus={(e) => e.currentTarget.select()}
                  />
                )}
                <p className={styles.pairingExpiry}>Expires in {minutesLeft(pairing.expiresAt)} min.</p>
                <ul className={styles.addresses}>
                  {state.addresses.map((address) => (
                    <li key={address}>{address}</li>
                  ))}
                </ul>
                <Button variant="ghost" onClick={cancelPairing}>Cancel</Button>
              </div>
            </div>
          ) : (
            <div className={styles.pairButtons}>
              <Button onClick={pairPhone}>Pair a phone</Button>
              <Button onClick={pairDesktop}>Pair a desktop</Button>
            </div>
          )}
        </Group>
      )}
      <Group label="Paired devices">
        {state.devices.length === 0 ? (
          <Row label="No devices paired yet" />
        ) : (
          <div className={styles.pairedDevices}>
            {state.devices.map((device) => {
              const handleRevoke = () => setRevokeTarget(device.id);
              return (
                <div key={device.id} className={styles.deviceRow}>
                  <div className={styles.deviceText}>
                    <span className={styles.deviceName}>{device.name}</span>
                    <span className={styles.deviceMeta}>
                      {device.desktop ? 'Desktop' : 'Phone'} · Paired {new Date(device.pairedAt).toLocaleDateString()}
                    </span>
                  </div>
                  <Button variant="danger" size="sm" onClick={handleRevoke}>Remove</Button>
                </div>
              );
            })}
          </div>
        )}
      </Group>
    </>
  );
};

export default DevicesSection;
