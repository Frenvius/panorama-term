export type AgentStatus = 'starting' | 'idle' | 'busy' | 'waiting' | 'crashed' | 'stopped';

export interface NativeSession {
  agent: string;
  id: string;
  boundAt: number;
}

export interface AgentExit {
  at: number;
  reason: string;
  code?: number;
}

export interface AgentLive {
  tileId: string;
  status: AgentStatus;
  live: boolean;
  statusSource: 'hook' | 'quiet' | 'none';
  contextPct?: number;
  costUsd?: number;
}

export interface PersistentAgent extends AgentLive {
  id: string;
  name: string;
  title: string;
  description: string;
  profile: string;
  model: string | null;
  effort: string | null;
  cwd: string;
  project: string | null;
  instructions?: string;
  autoStart: boolean;
  desired: 'running' | 'stopped';
  nativeSession: NativeSession | null;
  lastExit: AgentExit | null;
  crashed: boolean;
  createdAt: number;
  updatedAt: number;
  lastStartedAt: number | null;
}
