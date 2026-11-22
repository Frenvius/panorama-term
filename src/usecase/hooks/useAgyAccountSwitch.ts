import type { AgentAccount } from '~/domain/interfaces/account.interface';

import React from 'react';

import { errorText, showToast } from '~/usecase/util/toasts';
import { switchAgyAccount } from '~/usecase/util/accountSwitch';
import { accountsOf, accountName, useAgentAccounts, agentAccountsActions } from '~/usecase/hooks/useAgentAccounts';

export interface AgyAccountSwitchState {
  list: AgentAccount[];
  liveId: string | null;
  switchingId: string | null;
  select: (id: string) => Promise<void>;
}

export const useAgyAccountSwitch = (onSwitched?: () => void): AgyAccountSwitchState => {
  const { accounts, liveAgy } = useAgentAccounts();
  const [switchingId, setSwitchingId] = React.useState<string | null>(null);
  const list = accountsOf(accounts, 'antigravity');

  const select = async (id: string): Promise<void> => {
    const target = list.find((a) => a.id === id);
    if (!target || id === liveAgy || switchingId !== null) return;
    setSwitchingId(id);
    try {
      await switchAgyAccount(id, accountName(target));
      await agentAccountsActions.refresh();
      onSwitched?.();
    } catch (err) {
      showToast({ kind: 'error', message: 'Could not switch account', detail: errorText(err) });
    } finally {
      setSwitchingId(null);
    }
  };

  return { list, liveId: liveAgy, switchingId, select };
};
