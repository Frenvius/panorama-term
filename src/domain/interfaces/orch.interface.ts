export type RunStatus = 'active' | 'paused' | 'completed' | 'cancelled';

export type TaskStatus = 'pending' | 'dispatched' | 'verifying' | 'review' | 'completed' | 'failed' | 'blocked' | 'cancelled' | 'waiting';

export type DispatchStatus = 'starting' | 'running' | 'reported' | 'failed' | 'stopped' | 'stale';

export type Role = 'test' | 'review' | 'explore' | 'implement' | 'mechanical';

export type OrchestrateHint = 'off' | 'auto' | 'suggest';

export type FailureClass =
  | 'quota_exhausted'
  | 'rate_limited'
  | 'auth_failed'
  | 'model_unavailable'
  | 'launch_failed'
  | 'execution_failed'
  | 'setup_failed'
  | 'quality_failed'
  | 'unknown';

export type DispatchTrigger = 'initial' | 'fallback' | 'escalation' | 'resume' | 'manual';

export type DispatchBy = 'coordinator' | 'brain' | 'user';

export type WaitKind = 'quota' | 'availability' | 'intervention';

export type CandidateStatus = 'available' | 'rate_limited' | 'quota_exhausted' | 'auth_failed' | 'unavailable' | 'unknown';

export type CandidateConfidence = 'reliable' | 'partial';

export interface CandidateKey {
  agent: string;
  model?: string;
  account?: string;
}

export interface CandidateState {
  state: CandidateStatus;
  until?: number;
  source: string;
  confidence: CandidateConfidence;
  updatedAt: number;
  consecutive: number;
}

export interface CandidateEntry extends CandidateState {
  key: CandidateKey;
}

export interface SkippedCandidate {
  candidate: CandidateKey;
  state: CandidateStatus;
  reason: string;
}

export interface RoutingDecision {
  requested?: CandidateKey;
  effective: CandidateKey;
  reason: string;
  skipped: SkippedCandidate[];
}

export interface TaskWait {
  kind: WaitKind;
  resumeAt?: number;
  reason: string;
  candidate?: CandidateKey;
  sameSession: boolean;
  resumeId: string;
  since: number;
}

export interface Autonomy {
  operational: boolean;
  execution: boolean;
  integration: boolean;
}

export interface CoordinatorWait {
  resetAt?: number;
  since: number;
  reason: string;
  resumeId: string;
}

export interface UsageReportWindow {
  key: string;
  used: number;
  resetsAt: number | null;
}

export interface UsageReport {
  agent: string;
  accountId: string;
  windows: UsageReportWindow[];
}

export interface MetricsRow {
  agent: string;
  model: string | null;
  role: Role;
  dispatches: number;
  succeededReports: number;
  verificationPassed: number;
  qualityFailures: number;
  operationalFailures: Partial<Record<FailureClass, number>>;
  medianDurationMs: number | null;
  costUsd: number | null;
  lowSample: boolean;
}

export interface Limits {
  maxWorkers: number;
  maxAttempts: number;
  dispatchTimeoutMin: number;
  verifyTimeoutMin: number;
  stallQuietMin: number;
  maxOperationalAttempts?: number;
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
  autonomy?: Autonomy;
  coordinatorWait?: CoordinatorWait;
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
  routing?: number;
  qualityFloor?: number;
  qualityAttempts?: number;
  operationalAttempts?: number;
  pinned?: boolean;
  lastFailure?: FailureClass;
  wait?: TaskWait;
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
  live: boolean;
  agentStatus?: string;
  starting: boolean;
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
  account?: string;
  chainIndex?: number;
  trigger?: DispatchTrigger;
  decision?: RoutingDecision;
  failureClass?: FailureClass;
  by?: DispatchBy;
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

export interface AiTarget {
  provider: string;
  model: string;
}

export interface AiActionsSettings {
  default: AiTarget;
  overrides: {
    commitMessage?: AiTarget;
    tidy?: AiTarget;
  };
}

export interface TidyFrame {
  title: string;
  frameId?: string;
  path?: string;
  tileIds: string[];
}

export interface AiProgress {
  step: string;
  elapsedMs: number;
}

export interface TidyPlan {
  projectOrder: string[];
  frames: TidyFrame[];
}

export interface OrchSettings {
  coordinator: string;
  hiddenAgents: string[];
  providers: OrchProvider[];
  showWorkerTiles: boolean;
  roles: RoleChains;
  orchestrateHint: OrchestrateHint;
  customModels: Record<string, string[]>;
  aiActions: AiActionsSettings;
}

export interface OrchAgent {
  id: string;
  kind: 'builtin' | 'provider';
  label: string;
  available: boolean;
  hidden: boolean;
  models: string[];
  customModels: string[];
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
