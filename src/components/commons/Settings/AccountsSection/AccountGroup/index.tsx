import type { AgentAccountKind } from '~/domain/interfaces/account.interface';

import React from 'react';
import { Plus } from 'lucide-react';

import { useAccountLogin, accountLoginActions } from '~/usecase/hooks/useAccountLogin';
import { accountsOf, accountName, useAgentAccounts, agentAccountsActions } from '~/usecase/hooks/useAgentAccounts';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Select from '~/components/commons/Settings/Select';
import Button from '~/components/commons/Settings/Button';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';
import AccountRow from '../AccountRow';
import styles from './styles.module.scss';

interface AccountGroupProps {
  label: string;
  agent: AgentAccountKind;
}

const AccountGroup = ({ agent, label }: AccountGroupProps) => {
  const { accounts, defaults, liveAgy } = useAgentAccounts();
  const login = useAccountLogin(agent);
  const [removeId, setRemoveId] = React.useState<string | null>(null);

  const list = accountsOf(accounts, agent);
  const removing = list.find((a) => a.id === removeId);
  const defaultId = agent === 'antigravity' ? null : defaults[agent];
  const options = list.map((a) => ({ value: a.id, label: accountName(a) }));

  const addAccount = () => void accountLoginActions.start(agent);
  const cancelLogin = () => void accountLoginActions.cancel(agent);
  const closeConfirm = () => setRemoveId(null);

  const changeDefault = (id: string) => {
    if (agent === 'antigravity') return;
    void agentAccountsActions.setDefault({ id, agent });
  };

  const confirmRemove = () => {
    if (!removing) return;
    void agentAccountsActions.remove({ id: removing.id, agent });
    setRemoveId(null);
  };

  return (
    <Group
      label={label}
      action={
        <Button size="sm" icon={Plus} disabled={login.busy} onClick={addAccount}>
          Add account
        </Button>
      }
    >
      {removing && (
        <ConfirmDialog
          danger
          title="Remove account"
          confirmLabel="Remove"
          message={`Remove ${accountName(removing)}? Its saved login will be deleted.`}
          onConfirm={confirmRemove}
          onCancel={closeConfirm}
        />
      )}
      {defaultId !== null && (
        <Row label="Default for new terminals" description="New terminals start with this account.">
          <Select options={options} value={defaultId} onChange={changeDefault} />
        </Row>
      )}
      {list.length === 0 && <Row label="No accounts saved." description="Add one to switch between logins." />}
      {list.map((account) => (
        <AccountRow
          key={account.id}
          account={account}
          live={agent === 'antigravity' && liveAgy === account.id}
          onRemove={setRemoveId}
        />
      ))}
      {login.busy && (
        <div className={styles.waiting}>
          <div className={styles.waitingText}>
            <span className={styles.waitingTitle}>Waiting for login in the new terminal...</span>
            <span className={styles.waitingHint}>Close Settings to reach it. Close the terminal when you are done.</span>
          </div>
          <Button size="sm" variant="ghost" onClick={cancelLogin}>
            Cancel
          </Button>
        </div>
      )}
      {login.error && <p className={styles.error}>{login.error}</p>}
    </Group>
  );
};

export default AccountGroup;
