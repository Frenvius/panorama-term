export type RunStatus = 'active' | 'paused' | 'completed' | 'cancelled';

export type TaskStatus = 'pending' | 'dispatched' | 'verifying' | 'completed' | 'failed' | 'blocked' | 'cancelled';

export type DispatchStatus = 'starting' | 'running' | 'reported' | 'failed' | 'stopped' | 'stale';

export interface Limits {
  maxWorkers: number;
  maxAttempts: number;
  dispatchTimeoutMin: number;
  verifyTimeoutMin: number;
  stallQuietMin: number;
}

export interface Run {
  id: string;
  title: string;
  repo: string;
  baseRef?: string;
  baseSha?: string;
  setup?: string;
  coordinatorTile?: string;
  status: RunStatus;
  limits: Limits;
  createdAt: number;
  updatedAt: number;
}

export interface Task {
  id: string;
  title: string;
  spec: string;
  acceptance?: string;
  deps: string[];
  profile?: string;
  escalation: string[];
  verify: string[];
  review: boolean;
  status: TaskStatus;
  result?: string;
  integratedSha?: string;
  attempts: number;
  ready: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface VerificationResult {
  command: string;
  exitCode?: number;
  timedOut: boolean;
  tail: string[];
  headSha?: string;
  at: number;
}

export interface Dispatch {
  id: string;
  taskId: string;
  attempt: number;
  retryOf?: string;
  profile?: string;
  model?: string;
  tileId?: string;
  worktree?: string;
  branch?: string;
  status: DispatchStatus;
  outcome?: string;
  headSha?: string;
  filesModified: string[];
  summary?: string;
  startedAt: number;
  endedAt?: number;
  lastHeartbeat?: number;
  verifications?: VerificationResult[];
  costUsd?: number;
  costSource?: string;
}

export interface Gate {
  id: string;
  taskId: string;
  question: string;
  options: string[];
  resolution?: string;
  createdAt: number;
  resolvedAt?: number;
}

export interface RunSummary {
  run: Run;
  taskCounts: Record<string, number>;
}

export interface RunDetail {
  run: Run;
  tasks: Task[];
  dispatches: Dispatch[];
  gates: Gate[];
}
