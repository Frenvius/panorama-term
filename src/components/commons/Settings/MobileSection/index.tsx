import { X } from 'lucide-react';

import QrCode from '~/components/commons/QrCode';
import { useMobileAccess } from '~/usecase/hooks/useMobileAccess';
import { relayStatusLabel } from '~/usecase/converter/mobile.converter';
import ToggleOption from '~/components/commons/Settings/ToggleOption';
import settingsStyles from '~/components/commons/Settings/styles.module.scss';

import styles from './styles.module.scss';

const QR_SIZE = 192;

const minutesLeft = (expiresAt: number): number => Math.max(1, Math.ceil((expiresAt - Date.now()) / 60000));

const MobileSection = () => {
  const { state, pairing, error, toggleEnabled, toggleRemote, pair, cancelPairing, revoke } = useMobileAccess();

  return (
    <div className={settingsStyles.pane}>
      <div className={settingsStyles.paneHead}>
        <h2 className={settingsStyles.title}>Mobile</h2>
        <p className={settingsStyles.subtitle}>Watch and steer terminals from the Panorama phone app.</p>
      </div>
      {error && <p className={styles.error}>{error}</p>}
      {state && (
        <>
          <div className={settingsStyles.options}>
            <ToggleOption
              checked={state.enabled}
              label="Allow phone connections"
              onToggle={toggleEnabled}
              description={`Listen on port ${state.port} for paired phones on this network. Traffic is end-to-end encrypted.`}
            />
            {state.enabled && (
              <ToggleOption
                checked={state.remote}
                label="Allow access away from home"
                onToggle={toggleRemote}
                description={`Paired phones reach this computer through the Panorama relay from any network. The relay only forwards encrypted traffic. ${relayStatusLabel(state)}`}
              />
            )}
          </div>
          {state.enabled && !state.listening && (
            <p className={styles.error}>Port {state.port} could not be opened. Another app may be using it.</p>
          )}
          {state.enabled && (
            <div className={settingsStyles.group}>
              <p className={settingsStyles.groupLabel}>Pair a phone</p>
              {pairing ? (
                <div className={styles.pairing}>
                  <QrCode size={QR_SIZE} value={pairing.uri} />
                  <div className={styles.pairingInfo}>
                    <p className={settingsStyles.hint}>Scan with the Panorama app, or type the code.</p>
                    <span className={styles.code}>{pairing.code}</span>
                    <p className={settingsStyles.hint}>Expires in {minutesLeft(pairing.expiresAt)} min.</p>
                    <ul className={styles.addresses}>
                      {state.addresses.map((address) => (
                        <li key={address}>{address}</li>
                      ))}
                    </ul>
                    <button type="button" className={styles.button} onClick={cancelPairing}>
                      Cancel
                    </button>
                  </div>
                </div>
              ) : (
                <button type="button" className={styles.button} onClick={pair}>
                  Show pairing code
                </button>
              )}
              <p className={settingsStyles.hint}>
                The phone must reach this computer on the same network or over Tailscale. Windows may ask to allow
                Panorama through the firewall.
              </p>
            </div>
          )}
          <div className={settingsStyles.group}>
            <p className={settingsStyles.groupLabel}>Paired phones</p>
            {state.devices.length === 0 ? (
              <p className={settingsStyles.hint}>No phones paired yet.</p>
            ) : (
              <div className={styles.devices}>
                {state.devices.map((device) => (
                  <div key={device.id} className={styles.device}>
                    <span className={styles.deviceText}>
                      <span className={settingsStyles.optionLabel}>{device.name}</span>
                      <span className={settingsStyles.optionDesc}>
                        Paired {new Date(device.pairedAt).toLocaleDateString()}
                      </span>
                    </span>
                    <button
                      type="button"
                      className={styles.revoke}
                      onClick={() => revoke(device.id)}
                      aria-label={`Remove ${device.name}`}
                    >
                      <X size={14} strokeWidth={2} />
                    </button>
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
};

export default MobileSection;
