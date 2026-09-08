import { invoke } from '@tauri-apps/api/core';

import type { SessionSummary } from '~/domain/interfaces/claude.interface';

export const agentSessionSummary = (agent: string, sessionId: string, cwd?: string): Promise<SessionSummary | null> => {
  if (agent === 'pi') return invoke<SessionSummary | null>('pi_session_summary', { sessionId }).catch(() => null);
  return invoke<SessionSummary | null>('claude_session_summary', { sessionId, cwd: cwd ?? null }).catch(() => null);
};
