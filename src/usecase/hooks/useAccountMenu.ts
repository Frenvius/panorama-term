import type { AgentAccount, AgentAccountKind, PerTileAccountKind } from '~/domain/interfaces/account.interface';

import React from 'react';

import { getTileAccounts } from '~/adapter/accounts/accounts.client';
import { switchTileAccount, switchAgyAccount } from '~/usecase/util/accountSwitch';
import { accountsOf, accountName, useAgentAccounts, agentAccountsActions } from '~/usecase/hooks/useAgentAccounts';

const KINDS: string[] = ['antigravity', 'claude', 'codex'];

const kindOf = (agent: string | null): AgentAccountKind | null =>
  agent !== null && KINDS.includes(agent) ? (agent as AgentAccountKind) : null;

export interface AccountMenuState {
  visible: boolean;
  list: AgentAccount[];
  currentId: string | null;
  currentName: string;
  select: (id: string) => Promise<void>;
}

export const useAccountMenu = (tileId: string, agent: string | null): AccountMenuState => {
  const kind = kindOf(agent);
  const { accounts, defaults, liveAgy } = useAgentAccounts();
  const [tileCurrent, setTileCurrent] = React.useState<string | null>(null);
  const perTile = kind === 'claude' || kind === 'codex' ? kind : null;

  React.useEffect(() => {
    if (!perTile) return;
    let stale = false;
    getTileAccounts(tileId)
      .then((r) => !stale && setTileCurrent(r[perTile]))
      .catch(() => !stale && setTileCurrent(null));
    return () => {
      stale = true;
    };
  }, [tileId, perTile, accounts]);

  const list = kind ? accountsOf(accounts, kind) : [];
  const currentId = kind === 'antigravity' ? liveAgy : perTile ? (tileCurrent ?? defaults[perTile]) : null;
  const current = list.find((a) => a.id === currentId);

  const select = async (id: string): Promise<void> => {
    const target = list.find((a) => a.id === id);
    if (!kind || !target) return;
    if (kind === 'antigravity') {
      await switchAgyAccount(id, accountName(target));
      await agentAccountsActions.refresh();
      return;
    }
    await switchTileAccount({ id, tileId, agent: kind as PerTileAccountKind });
    setTileCurrent(id);
  };

  return {
    list,
    select,
    currentId,
    visible: list.length >= 2,
    currentName: current ? accountName(current) : 'account'
  };
};
