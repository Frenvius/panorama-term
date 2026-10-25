import type { ClaudeState } from '~/domain/interfaces/pty.interface';

type StateKey = keyof ClaudeState;

const TRACKER_SESSION_KEYS: readonly StateKey[] = ['status', 'linesAdded', 'linesRemoved', 'contextTokens'];

const SESSION_KEYS: readonly StateKey[] = [...TRACKER_SESSION_KEYS, 'dialog', 'costUsd', 'suggestion', 'contextPercent'];

export const mergeClaudeState = (prev: ClaudeState, incoming: ClaudeState): ClaudeState => {
  const { reset, sessionReset, ...rest } = incoming;
  const dropped = sessionReset ? SESSION_KEYS : reset ? TRACKER_SESSION_KEYS : [];
  const kept = { ...prev };
  for (const key of dropped) delete kept[key];
  return { ...kept, ...rest };
};
