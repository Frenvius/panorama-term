export type RunSessionState = 'none' | 'running' | 'exited';

export type RunPhase = 'starting' | 'restarting' | 'stopping' | 'killing';

export interface RunStatus {
  state: RunSessionState;
  cmd?: string;
  pid?: number;
  ready?: boolean;
  lastLine?: string;
  readyReason?: string | null;
  stoppedByUser?: boolean;
  phase?: 'stopping' | 'killing' | null;
  exitCode?: number | null;
  totalLines?: number;
  sessionId?: string;
  error?: string;
}
