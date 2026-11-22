import React from 'react';
import { Check, ChevronDown, User } from 'lucide-react';

import { isTileBusy } from '~/usecase/util/accountSwitch';
import { errorText, showToast } from '~/usecase/util/toasts';
import { requestSettings } from '~/usecase/util/settingsNav';
import { accountName } from '~/usecase/hooks/useAgentAccounts';
import { useAccountMenu } from '~/usecase/hooks/useAccountMenu';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';
import styles from './styles.module.scss';

interface AccountMenuProps {
  tileId: string;
  agent: string | null;
}

const AGENT_NAME: Record<string, string> = { codex: 'Codex', claude: 'Claude', antigravity: 'Antigravity' };

const AccountMenu = ({ tileId, agent }: AccountMenuProps) => {
  const { visible, list, currentId, currentName, select } = useAccountMenu(tileId, agent);
  const [open, setOpen] = React.useState(false);
  const [confirmId, setConfirmId] = React.useState<string | null>(null);
  const rootRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!open) return;
    const handleOutside = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', handleOutside, true);
    return () => document.removeEventListener('mousedown', handleOutside, true);
  }, [open]);

  if (!visible) return null;

  const toggleMenu = () => setOpen((o) => !o);
  const cancelConfirm = () => setConfirmId(null);
  const manageAccounts = () => {
    setOpen(false);
    requestSettings('accounts');
  };

  const runSwitch = async (id: string) => {
    try {
      await select(id);
    } catch (err) {
      showToast({ kind: 'error', message: 'Could not switch account', detail: errorText(err) });
    }
  };

  const pickAccount = (id: string) => () => {
    setOpen(false);
    if (id === currentId) return;
    if (agent !== 'antigravity' && isTileBusy(tileId)) {
      setConfirmId(id);
      return;
    }
    void runSwitch(id);
  };

  const confirmSwitch = () => {
    const id = confirmId;
    setConfirmId(null);
    if (id) void runSwitch(id);
  };

  const target = list.find((a) => a.id === confirmId);
  const agentName = AGENT_NAME[agent ?? ''] ?? 'The agent';

  return (
    <div ref={rootRef} className={styles.root}>
      <button type="button" className={styles.chip} title="Switch account" onClick={toggleMenu}>
        <User size={9} />
        <span className={styles.label}>{currentName}</span>
        <ChevronDown size={9} />
      </button>
      {open && (
        <div className={styles.menu}>
          {list.map((account) => (
            <button key={account.id} type="button" className={styles.item} onClick={pickAccount(account.id)}>
              <span className={styles.tick}>{account.id === currentId && <Check size={10} />}</span>
              {accountName(account)}
            </button>
          ))}
          <div className={styles.divider} />
          <button type="button" className={styles.item} onClick={manageAccounts}>
            <span className={styles.tick} />
            Manage accounts...
          </button>
        </div>
      )}
      {target && (
        <ConfirmDialog
          title="Switch account"
          confirmLabel="Restart"
          message={`${agentName} is working. Restart now on ${accountName(target)}?`}
          onConfirm={confirmSwitch}
          onCancel={cancelConfirm}
        />
      )}
    </div>
  );
};

export default AccountMenu;
