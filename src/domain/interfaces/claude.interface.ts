export interface SessionTurn {
  role: 'user' | 'assistant';
  text: string;
  tools: string[];
}

export interface SessionSummary {
  sessionId: string;
  cwd: string | null;
  branch: string | null;
  model: string | null;
  version: string | null;
  endedAt: string | null;
  promptCount: number;
  partial: boolean;
  turns: SessionTurn[];
}
