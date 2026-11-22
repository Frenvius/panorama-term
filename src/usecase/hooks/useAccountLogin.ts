import type { AgentAccountKind } from '~/domain/interfaces/account.interface';

import React from 'react';

import { agentAccountsActions } from '~/usecase/hooks/useAgentAccounts';
import { createTileId, requestOpenTerminal } from '~/usecase/util/openTerminalRequest';
import { cancelAccountLogin, getAccountLoginStatus, startAccountLogin } from '~/adapter/accounts/accounts.client';

export interface AccountLoginProgress {
  busy: boolean;
  pendingId: string | null;
  error: string | null;
}

const POLL_MS = 2000;
const IDLE: AccountLoginProgress = { busy: false, pendingId: null, error: null };

const FAILURE_TEXT: Record<string, string> = { duplicate: 'This account is already added' };

let states: Partial<Record<AgentAccountKind, AccountLoginProgress>> = {};

const timers = new Map<AgentAccountKind, number>();
const listeners = new Set<() => void>();

const notify = (): void => {
  for (const fn of listeners) fn();
};

const setState = (agent: AgentAccountKind, next: AccountLoginProgress): void => {
  states = { ...states, [agent]: next };
  notify();
};

const stopPolling = (agent: AgentAccountKind): void => {
  window.clearInterval(timers.get(agent));
  timers.delete(agent);
};

const errorText = (err: unknown): string => (err instanceof Error ? err.message : String(err));

const poll = async (agent: AgentAccountKind, pendingId: string): Promise<void> => {
  try {
    const reply = await getAccountLoginStatus(pendingId);
    if (reply.state === 'waiting') return;
    stopPolling(agent);
    if (reply.state === 'done') {
      setState(agent, IDLE);
      await agentAccountsActions.refresh();
      return;
    }
    const raw = reply.error ?? 'Login failed';
    setState(agent, { busy: false, pendingId: null, error: FAILURE_TEXT[raw] ?? raw });
  } catch (err) {
    stopPolling(agent);
    setState(agent, { busy: false, pendingId: null, error: errorText(err) });
  }
};

const startPolling = (agent: AgentAccountKind, pendingId: string): void => {
  stopPolling(agent);
  timers.set(agent, window.setInterval(() => void poll(agent, pendingId), POLL_MS));
};

export const accountLoginActions = {
  start: async (agent: AgentAccountKind): Promise<void> => {
    setState(agent, { busy: true, pendingId: null, error: null });
    try {
      const tileId = createTileId();
      const { pendingId, command } = await startAccountLogin({ agent, tileId });
      setState(agent, { busy: true, pendingId, error: null });
      requestOpenTerminal({ tileId, command });
      startPolling(agent, pendingId);
    } catch (err) {
      setState(agent, { busy: false, pendingId: null, error: errorText(err) });
    }
  },

  cancel: async (agent: AgentAccountKind): Promise<void> => {
    const pendingId = states[agent]?.pendingId;
    stopPolling(agent);
    setState(agent, IDLE);
    if (!pendingId) return;
    try {
      await cancelAccountLogin(pendingId);
    } catch (err) {
      setState(agent, { busy: false, pendingId: null, error: errorText(err) });
    }
  }
};

const subscribe = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const useAccountLogin = (agent: AgentAccountKind): AccountLoginProgress => {
  const snapshot = React.useSyncExternalStore(subscribe, () => states[agent] ?? IDLE);
  return snapshot;
};
