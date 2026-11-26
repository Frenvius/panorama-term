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
  coordinates?: string[];
  canCoordinate?: boolean;
  waits?: AgentWaitSummary[];
  routines?: { count: number; enabled: number; nextRunLocal: string | null; lastAt: number | null };
}

export type RoutineOutcome = 'reported' | 'no_update' | 'failed' | 'timeout' | 'skipped';

export type RoutineTrigger =
  | { kind: 'cron'; expr: string }
  | { kind: 'every'; ms: number; anchor: number }
  | { kind: 'hook' }
  | { kind: 'manual' };

export interface Routine {
  id: string;
  agentId: string;
  agentName: string | null;
  name: string;
  instruction: string;
  trigger: RoutineTrigger;
  triggerText: string;
  enabled: boolean;
  pausedReason: string | null;
  missed: 'catchUp' | 'skip';
  overlap: 'skip' | 'queue';
  session: 'keep' | 'fresh';
  reportTo: { kind: 'user' } | { kind: 'agent'; id: string } | { kind: 'none' };
  maxPerDay: number;
  maxCostDay: number | null;
  runTimeoutMs: number;
  nextRunLocal: string | null;
  last: { outcome: RoutineOutcome | null; at: number | null; summary?: string | null; reason?: string } | null;
}

export interface RoutineRun {
  routineId: string;
  n: number;
  firedAt: number;
  trigger: string;
  deliveredAt: number | null;
  endedAt: number | null;
  outcome: RoutineOutcome | null;
  summary: string | null;
  costUsd: number | null;
}

export interface RoutineSkip {
  routineId: string;
  at: number;
  reason: string;
  count: number;
}

export interface AgentWaitSummary {
  id: string;
  on: string[];
  note: string;
  timeoutAt: number | null;
}

export type WaitStatus = 'open' | 'fired' | 'cancelled' | 'expired';

export interface AgentWait {
  id: string;
  on: string[];
  note: string;
  createdAt: number;
  timeoutAt: number | null;
  status: WaitStatus;
  fired: { at: number; summary: string; wake: boolean; started: boolean } | null;
  cancelledBy: string | null;
}

export type Party =
  | { kind: 'agent'; id: string }
  | { kind: 'tile'; id: string }
  | { kind: 'user' }
  | { kind: 'brain' };

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
  kind?: 'event' | 'orch' | 'routine';
  routineName?: string | null;
  key?: string;
  truncated?: boolean;
}
