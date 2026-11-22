import type { AccountDefaults, AgentAccount, AgentAccountKind, PerTileAccountKind } from '~/domain/interfaces/account.interface';

import React from 'react';

import {
  listAccounts,
  removeAccount,
  renameAccount,
  setDefaultAccount
} from '~/adapter/accounts/accounts.client';

interface AgentAccountsState {
  loaded: boolean;
  liveAgy: string | null;
  error: string | null;
  accounts: AgentAccount[];
  defaults: AccountDefaults;
}

const EMPTY: AgentAccountsState = {
  loaded: false,
  liveAgy: null,
  error: null,
  accounts: [],
  defaults: { codex: 'system', claude: 'system' }
};

let state: AgentAccountsState = EMPTY;

const listeners = new Set<() => void>();

const notify = (): void => {
  for (const fn of listeners) fn();
};

const getSnapshot = (): AgentAccountsState => state;

const fail = (err: unknown): void => {
  state = { ...state, error: err instanceof Error ? err.message : String(err) };
  notify();
};

const refresh = (): Promise<void> =>
  listAccounts()
    .then((r) => {
      state = { loaded: true, error: null, accounts: r.accounts, defaults: r.defaults, liveAgy: r.liveAgy };
      notify();
    })
    .catch(fail);

let subscriberCount = 0;

const subscribe = (fn: () => void): (() => void) => {
  listeners.add(fn);
  subscriberCount++;
  if (subscriberCount === 1) void refresh();
  return () => {
    listeners.delete(fn);
    subscriberCount--;
  };
};

const mutate = (task: Promise<unknown>): Promise<void> => task.then(refresh).catch(fail);

interface AccountRef {
  id: string;
  agent: AgentAccountKind;
}

export const agentAccountsActions = {
  refresh,

  remove: (ref: AccountRef): Promise<void> => mutate(removeAccount(ref)),

  rename: (ref: AccountRef & { label: string }): Promise<void> => mutate(renameAccount(ref)),

  setDefault: (ref: { id: string; agent: PerTileAccountKind }): Promise<void> => mutate(setDefaultAccount(ref))
};

export const useAgentAccounts = (): AgentAccountsState => React.useSyncExternalStore(subscribe, getSnapshot);

export const accountsOf = (accounts: AgentAccount[], agent: AgentAccountKind): AgentAccount[] =>
  accounts.filter((a) => a.agent === agent);

export const accountName = (account: AgentAccount): string => account.email ?? account.label;
