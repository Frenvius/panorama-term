import type { AgentAccount } from '~/domain/interfaces/account.interface';

import React from 'react';
import { Pencil, Trash2 } from 'lucide-react';

import { errorText, showToast } from '~/usecase/util/toasts';
import { switchAgyAccount } from '~/usecase/util/accountSwitch';
import { accountName, agentAccountsActions } from '~/usecase/hooks/useAgentAccounts';
import Button from '~/components/commons/Settings/Button';
import styles from './styles.module.scss';

interface AccountRowProps {
  live: boolean;
  account: AgentAccount;
  onRemove: (id: string) => void;
}

const AccountRow = ({ account, live, onRemove }: AccountRowProps) => {
  const [draft, setDraft] = React.useState<string | null>(null);
  const [switching, setSwitching] = React.useState(false);

  const startRename = () => setDraft(account.label);
  const cancelRename = () => setDraft(null);
  const changeDraft = (e: React.ChangeEvent<HTMLInputElement>) => setDraft(e.target.value);
  const requestRemove = () => onRemove(account.id);

  const commitRename = () => {
    const label = draft?.trim();
    setDraft(null);
    if (!label || label === account.label) return;
    void agentAccountsActions.rename({ id: account.id, agent: account.agent, label });
  };

  const handleKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') commitRename();
    else if (e.key === 'Escape') {
      e.stopPropagation();
      cancelRename();
    }
  };

  const activate = async () => {
    setSwitching(true);
    try {
      await switchAgyAccount(account.id, accountName(account));
      await agentAccountsActions.refresh();
    } catch (err) {
      showToast({ kind: 'error', message: 'Could not switch account', detail: errorText(err) });
    } finally {
      setSwitching(false);
    }
  };

  const handleActivate = () => void activate();
  const showEmail = account.email !== null && account.email !== account.label;

  return (
    <div className={styles.row}>
      <div className={styles.text}>
        {draft === null ? (
          <span className={styles.label}>{account.label}</span>
        ) : (
          <input
            autoFocus
            value={draft}
            className={styles.input}
            aria-label="Account name"
            onChange={changeDraft}
            onBlur={commitRename}
            onKeyDown={handleKey}
          />
        )}
        {showEmail && <span className={styles.email}>{account.email}</span>}
      </div>
      {account.system && <span className={styles.badge}>System</span>}
      {live && <span className={`${styles.badge} ${styles.live}`}>Live</span>}
      {account.agent === 'antigravity' && !live && (
        <Button size="sm" busy={switching} busyLabel="Switching..." onClick={handleActivate}>
          Activate
        </Button>
      )}
      <button type="button" className={styles.iconBtn} aria-label="Rename account" onClick={startRename}>
        <Pencil size={13} strokeWidth={1.75} />
      </button>
      {!account.system && (
        <button type="button" className={`${styles.iconBtn} ${styles.danger}`} aria-label="Remove account" onClick={requestRemove}>
          <Trash2 size={13} strokeWidth={1.75} />
        </button>
      )}
    </div>
  );
};

export default AccountRow;
