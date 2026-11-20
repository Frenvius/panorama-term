export type RunStatus = 'active' | 'paused' | 'completed' | 'cancelled';

export type TaskStatus = 'pending' | 'dispatched' | 'verifying' | 'review' | 'completed' | 'failed' | 'blocked' | 'cancelled';

export type DispatchStatus = 'starting' | 'running' | 'reported' | 'failed' | 'stopped' | 'stale';

export type Role = 'test' | 'review' | 'explore' | 'implement' | 'mechanical';

export type OrchestrateHint = 'off' | 'auto' | 'suggest';

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
  role?: Role;
  profile?: string;
  escalation: string[];
  verify: string[];
  review: boolean;
  owner?: string;
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
  waiting?: boolean;
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

export interface OrchProvider {
  id: string;
  preset: string;
  label: string;
  baseUrl: string;
  mainModel: string;
  fastModel: string;
  hasKey: boolean;
}

export interface OrchPreset {
  id: string;
  label: string;
  baseUrl: string;
  mainModel: string;
  fastModel: string;
}

export interface RoleChains {
  test: string[];
  review: string[];
  explore: string[];
  implement: string[];
  mechanical: string[];
}

export interface CostEntry {
  usd: number;
  reported: number;
  dispatches: number;
}

export interface OrchSettings {
  coordinator: string;
  hiddenAgents: string[];
  providers: OrchProvider[];
  showWorkerTiles: boolean;
  roles: RoleChains;
  orchestrateHint: OrchestrateHint;
}

export interface OrchAgent {
  id: string;
  kind: 'builtin' | 'provider';
  label: string;
  available: boolean;
  hidden: boolean;
  models: string[];
}

export interface RunSummary {
  run: Run;
  taskCounts: Record<string, number>;
  openGates: number;
  coordinatorCostUsd?: number;
  costByRole?: Record<string, CostEntry>;
  costByModel?: Record<string, CostEntry>;
}

export interface RunDetail {
  run: Run;
  tasks: Task[];
  dispatches: Dispatch[];
  gates: Gate[];
  coordinatorCostUsd?: number;
  costByRole?: Record<string, CostEntry>;
  costByModel?: Record<string, CostEntry>;
}
