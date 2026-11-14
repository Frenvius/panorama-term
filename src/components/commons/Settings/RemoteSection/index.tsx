import React from 'react';
import { connectRemote, disconnectRemote, forgetRemote, pairRemote } from '~/adapter/remote/remote.client';
import { useRemoteStatus } from '~/usecase/hooks/useRemoteStatus';
import { remoteLabel } from '~/usecase/util/remote';
import settingsStyles from '../styles.module.scss';
import styles from '../MobileSection/styles.module.scss';

const RemoteSection = () => {
  const status = useRemoteStatus();
  const [uri, setUri] = React.useState('');
  const [name, setName] = React.useState('Desktop');
  const [error, setError] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError('');
    try {
      await action();
      location.reload();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };
  const connected = status && status.status !== 'disconnected';
  return (
    <div className={settingsStyles.pane}>
      <div className={settingsStyles.paneHead}>
        <h2 className={settingsStyles.title}>Remote host</h2>
        <p className={settingsStyles.subtitle}>Use the workspace and terminals of a paired desktop on your network.</p>
      </div>
      <div className={settingsStyles.group}>
        <p className={settingsStyles.groupLabel}>Connection</p>
        <p className={settingsStyles.hint}>{status ? remoteLabel(status) : 'Loading...'}</p>
        {error && <p className={styles.error}>{error}</p>}
        <div className={styles.actions}>
          {connected && (
            <button className={styles.button} disabled={busy} onClick={() => void run(disconnectRemote)}>
              Disconnect
            </button>
          )}
          {!connected && status?.remembered && (
            <button className={styles.button} disabled={busy} onClick={() => void run(connectRemote)}>
              Connect
            </button>
          )}
          {status?.remembered && (
            <button className={styles.button} disabled={busy} onClick={() => void run(forgetRemote)}>
              Forget
            </button>
          )}
        </div>
      </div>
      {!connected && (
        <div className={settingsStyles.group}>
          <p className={settingsStyles.groupLabel}>Pair a desktop</p>
          <p className={settingsStyles.hint}>
            On the host, open Settings → Mobile → Pair a desktop, then paste its pairing URI here.
          </p>
          <textarea
            className={styles.uri}
            aria-label="Pairing URI"
            placeholder="panorama-mobile://pair..."
            value={uri}
            onChange={(e) => setUri(e.target.value)}
          />
          <input
            className={styles.uri}
            style={{ minHeight: 0 }}
            aria-label="Device name"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
          <button className={styles.button} disabled={busy || !uri.trim()} onClick={() => void run(() => pairRemote(uri, name))}>
            Pair and connect
          </button>
        </div>
      )}
    </div>
  );
};
export default RemoteSection;
