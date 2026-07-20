export type RunSessionState = 'none' | 'running' | 'exited';

export interface RunStatus {
  state: RunSessionState;
  cmd?: string;
  pid?: number;
  exitCode?: number | null;
  totalLines?: number;
  sessionId?: string;
  error?: string;
}
