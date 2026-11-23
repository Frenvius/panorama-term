import type { AiProgress, AiActionsSettings, OrchAgent, OrchPreset, OrchProvider, OrchSettings, RunDetail, RunSummary, RoleChains, OrchestrateHint, TidyPlan, Autonomy, CandidateEntry, CandidateKey, MetricsRow, UsageReport } from '~/domain/interfaces/orch.interface';

import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';

interface OkResponse {
  ok: boolean;
  error?: { code: string; message: string };
}

export class OrchError extends Error {
  code?: string;
  constructor(message: string, code?: string) {
    super(message);
    this.name = 'OrchError';
    this.code = code;
  }
}

const post = async <T>(command: string, body: unknown): Promise<T> => {
  let res: Response;
  try {
    res = await fetch(`${SIDECAR_HTTP}/orch/${command}`, { method: 'POST', body: JSON.stringify(body) });
  } catch {
    throw new Error('Panorama brain is outdated: restart it to enable orchestration');
  }
  if (!res.ok) throw new Error('Panorama brain is outdated: restart it to enable orchestration');
  let json: OkResponse & T;
  try {
    json = (await res.json()) as OkResponse & T;
  } catch {
    throw new Error('Panorama brain is outdated: restart it to enable orchestration');
  }
  if (!json.ok) throw new OrchError(json.error?.message ?? command, json.error?.code);
  return json;
};

export const listRuns = (): Promise<{ ok: boolean; runs: RunSummary[] }> => post('run-list', {});

export const showRun = (runId: string): Promise<{ ok: boolean } & RunDetail> => post('run-show', { runId, full: true });

interface ResolveGateOpts {
  runId: string;
  gateId: string;
  resolution: string;
}

export const resolveGate = (opts: ResolveGateOpts): Promise<void> => post('gate-resolve', opts).then(() => undefined);

export const getSettings = (): Promise<{ ok: boolean; settings: OrchSettings; presets: OrchPreset[] }> =>
  post('settings-get', {});

export interface SetSettingsOpts {
  coordinator?: string;
  hiddenAgents?: string[];
  showWorkerTiles?: boolean;
  roles?: Partial<RoleChains>;
  orchestrateHint?: OrchestrateHint;
  customModels?: Record<string, string[]>;
  aiActions?: Partial<AiActionsSettings>;
}

export const setSettings = (opts: SetSettingsOpts): Promise<{ ok: boolean; settings: OrchSettings }> =>
  post('settings-set', opts);

export interface SaveProviderOpts {
  id?: string;
  preset: string;
  label: string;
  baseUrl: string;
  mainModel: string;
  fastModel: string;
  apiKey?: string;
}

export const saveProvider = (opts: SaveProviderOpts): Promise<{ ok: boolean; provider: OrchProvider }> =>
  post('provider-save', opts);

export const deleteProvider = (id: string): Promise<void> =>
  post('provider-delete', { id }).then(() => undefined);

export const testProvider = (id: string): Promise<{ ok: boolean; reachable: boolean; status?: number; message: string }> =>
  post('provider-test', { id });

export const listAgents = (opts?: { refresh?: boolean }): Promise<{ ok: boolean; agents: OrchAgent[] }> =>
  post('agent-list', opts ?? {});

export const setCoordinator = (tileId: string, on: boolean): Promise<{ ok: boolean }> =>
  post('coordinator-set', { tileId, on });

export const listCoordinators = (): Promise<{ ok: boolean; tiles: string[] }> => post('coordinator-list', {});

export interface CancelTaskOpts {
  runId: string;
  taskId: string;
}

export const cancelTask = (opts: CancelTaskOpts): Promise<void> =>
  post('task-cancel', opts).then(() => undefined);

export interface CreateRunOpts {
  repo: string;
  title: string;
}

export const createRun = (opts: CreateRunOpts): Promise<{ ok: boolean; run: { id: string; title: string } }> =>
  post('run-create', opts);

export const updateRun = (runId: string, title: string): Promise<void> =>
  post('run-update', { runId, title }).then(() => undefined);

export const setRunAutonomy = (runId: string, autonomy: Autonomy): Promise<void> => {
  const names = (Object.keys(autonomy) as (keyof Autonomy)[]).filter((k) => autonomy[k]);
  return post('run-update', { runId, autonomy: names.join(',') }).then(() => undefined);
};

export const listCandidates = (): Promise<{ ok: boolean; candidates: CandidateEntry[] }> => post('candidate-list', {});

export const clearCandidate = (key: CandidateKey): Promise<void> =>
  post('candidate-clear', key).then(() => undefined);

export const reportUsage = (report: UsageReport): Promise<void> =>
  post('usage-report', report).then(() => undefined);

export const getMetrics = (): Promise<{ ok: boolean; rows: MetricsRow[] }> => post('metrics', {});

export const setRunStatus = (runId: string, status: string): Promise<void> =>
  post('run-set-status', { runId, status }).then(() => undefined);

export const deleteRun = (runId: string): Promise<{ kept: { path: string; reason: string }[] }> =>
  post('run-delete', { runId });

export const setRunCoordinator = (runId: string, tileId: string | null): Promise<void> =>
  post('run-set-coordinator', { runId, tileId }).then(() => undefined);

export interface CreateTaskOpts {
  runId: string;
  title: string;
  spec: string;
  profile?: string;
  review?: boolean;
  by?: string;
}

export const createTask = (opts: CreateTaskOpts): Promise<{ ok: boolean; task: { id: string } }> =>
  post('task-create', opts);

export interface UpdateTaskOpts {
  runId: string;
  taskId: string;
  status?: string;
  owner?: 'user' | 'coordinator';
  spec?: string;
  profile?: string;
  review?: boolean;
  by?: string;
}

export const updateTask = (opts: UpdateTaskOpts): Promise<void> =>
  post('task-update', opts).then(() => undefined);

export interface StartWorkerOpts {
  runId: string;
  taskId: string;
  by: string;
  retryOf?: string;
  stopLive?: boolean;
}

export const startWorker = (opts: StartWorkerOpts): Promise<void> =>
  post('worker-start', opts).then(() => undefined);

export interface ResolveConflictOpts {
  runId: string;
  taskId: string;
}

export const resolveConflict = (opts: ResolveConflictOpts): Promise<void> =>
  post('conflict-resolve', opts).then(() => undefined);

export interface WorkerStopOpts {
  runId: string;
  dispatchId: string;
}

export const workerStop = (opts: WorkerStopOpts): Promise<void> =>
  post('worker-stop', opts).then(() => undefined);

export const aiCommitMessage = (repo: string, files: string[]): Promise<string> =>
  post<{ message: string }>('ai-commit-message', { repo, files }).then((r) => r.message);

export const aiTidy = (snapshot: string): Promise<TidyPlan> =>
  post<{ plan: TidyPlan }>('ai-tidy', { snapshot }).then((r) => r.plan);

export const aiProgress = (action: string): Promise<AiProgress | null> =>
  post<{ progress: AiProgress | null }>('ai-progress', { action }).then((r) => r.progress);
