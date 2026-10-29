import { invoke } from '@tauri-apps/api/core';

import type { SessionSummary, ConversationEntry } from '~/domain/interfaces/claude.interface';

export const agentSessionSummary = (agent: string, sessionId: string, cwd?: string): Promise<SessionSummary | null> => {
  if (agent === 'pi') return invoke<SessionSummary | null>('pi_session_summary', { sessionId }).catch(() => null);
  return invoke<SessionSummary | null>('claude_session_summary', { sessionId, cwd: cwd ?? null }).catch(() => null);
};

export const agentHistory = (agent: string, cwd: string): Promise<ConversationEntry[]> =>
  invoke<ConversationEntry[]>('agent_history', { agent, cwd });
