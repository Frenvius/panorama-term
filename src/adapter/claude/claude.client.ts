import { invoke } from '@tauri-apps/api/core';

import type { SessionSummary } from '~/domain/interfaces/claude.interface';

export const claudeSessionSummary = (sessionId: string, cwd?: string): Promise<SessionSummary | null> =>
  invoke<SessionSummary | null>('claude_session_summary', { sessionId, cwd: cwd ?? null }).catch(() => null);
