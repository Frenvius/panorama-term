import React from 'react';

import { connectRemote, disconnectRemote, forgetRemote, pairRemote } from '~/adapter/remote/remote.client';
import { useRemoteStatus } from '~/usecase/hooks/useRemoteStatus';
import { remoteLabel } from '~/usecase/util/remote';
import Group from '~/components/commons/Settings/Group';
import Button from '~/components/commons/Settings/Button';
import Field, { Input, Textarea } from '~/components/commons/Settings/Field';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';

import styles from './styles.module.scss';

const RemoteSection = () => {
  const status = useRemoteStatus();
  const [uri, setUri] = React.useState('');
  const [name, setName] = React.useState('Desktop');
  const [error, setError] = React.useState('');
  const [busy, setBusy] = React.useState(false);
  const [confirmForget, setConfirmForget] = React.useState(false);

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

  const disconnect = () => void run(disconnectRemote);
  const connect = () => void run(connectRemote);
  const openForget = () => setConfirmForget(true);
  const closeForget = () => setConfirmForget(false);

  const doForget = () => {
    setConfirmForget(false);
    void run(forgetRemote);
  };

  const doPair = () => void run(() => pairRemote(uri, name));

  const connected = status && status.status !== 'disconnected';

  return (
    <>
      {confirmForget && (
        <ConfirmDialog
          title="Forget remote host"
          message="Remove the saved connection? You will need to pair again to reconnect."
          confirmLabel="Forget"
          danger
          onConfirm={doForget}
          onCancel={closeForget}
        />
      )}
      <Group label="Connection">
        <div className={styles.statusRow}>
          <span className={styles.statusLabel}>{status ? remoteLabel(status) : 'Loading...'}</span>
          <div className={styles.statusActions}>
            {connected && (
              <Button disabled={busy} busy={busy} busyLabel="Working..." onClick={disconnect}>
                Disconnect
              </Button>
            )}
            {!connected && status?.remembered && (
              <Button disabled={busy} busy={busy} busyLabel="Working..." onClick={connect}>
                Connect
              </Button>
            )}
            {status?.remembered && (
              <Button variant="danger" disabled={busy} onClick={openForget}>
                Forget
              </Button>
            )}
          </div>
        </div>
        {error && <p className={styles.error}>{error}</p>}
      </Group>
      {!connected && (
        <Group label="Pair a desktop">
          <div className={styles.pairForm}>
            <Field label="Pairing URI" hint="On the host, open Settings - Devices - Pair a desktop, then paste its pairing URI here.">
              <Textarea
                value={uri}
                onChange={setUri}
                placeholder="panorama-mobile://pair..."
                ariaLabel="Pairing URI"
              />
            </Field>
            <Field label="Device name">
              <Input value={name} onChange={setName} ariaLabel="Device name" />
            </Field>
            <Button variant="primary" disabled={busy || !uri.trim()} busy={busy} busyLabel="Connecting..." onClick={doPair}>
              Pair and connect
            </Button>
          </div>
        </Group>
      )}
    </>
  );
};

export default RemoteSection;
