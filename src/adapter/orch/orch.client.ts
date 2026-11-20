import type { RunDetail, RunSummary } from '~/domain/interfaces/orch.interface';

import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';

interface OkResponse {
  ok: boolean;
  error?: { code: string; message: string };
}

const post = async <T>(command: string, body: unknown): Promise<T> => {
  const res = await fetch(`${SIDECAR_HTTP}/orch/${command}`, { method: 'POST', body: JSON.stringify(body) });
  const json = (await res.json()) as OkResponse & T;
  if (!json.ok) throw new Error(json.error?.message ?? command);
  return json;
};

export const listRuns = (): Promise<{ ok: boolean; runs: RunSummary[] }> => post('run-list', {});

export const showRun = (runId: string): Promise<{ ok: boolean } & RunDetail> => post('run-show', { runId });

interface ResolveGateOpts {
  runId: string;
  gateId: string;
  resolution: string;
}

export const resolveGate = (opts: ResolveGateOpts): Promise<void> => post('gate-resolve', opts).then(() => undefined);
