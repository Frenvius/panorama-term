export type RunSessionState = 'none' | 'running' | 'exited';

export type RunPhase = 'starting' | 'restarting' | 'stopping' | 'killing';

export interface RunStatus {
  state: RunSessionState;
  cmd?: string;
  pid?: number;
  exitCode?: number | null;
  totalLines?: number;
  sessionId?: string;
  error?: string;
}
