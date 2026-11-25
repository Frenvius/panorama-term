export type AgentStatus = 'starting' | 'idle' | 'busy' | 'waiting' | 'crashed' | 'stopped';

export interface NativeSession {
  agent: string;
  id: string;
  boundAt: number;
  model?: string | null;
}

export interface AgentExit {
  at: number;
  reason: string;
  code?: number | null;
  detail?: string;
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
  queued: number;
  awaitingReplies: number;
  startedBy?: string;
}

export type Party =
  | { kind: 'agent'; id: string }
  | { kind: 'tile'; id: string }
  | { kind: 'user' };

export type MessageStatus = 'queued' | 'delivered' | 'dropped' | 'failed';

export interface AgentMessage {
  id: string;
  from: Party;
  to: Party;
  fromName: string;
  fromLabel: string;
  toLabel: string;
  text: string;
  expectsReply: boolean;
  replyTo: string | null;
  thread: string;
  hops: number;
  at: number;
  seq: number;
  status: MessageStatus;
  deliveredAt: number | null;
  repliedBy: string | null;
  reason: string | null;
  read: boolean;
  notice: boolean;
  truncated?: boolean;
}
