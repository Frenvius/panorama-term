import type { PersistentAgent } from '~/domain/interfaces/agents.interface';

import { SIDECAR_HTTP } from '~/adapter/pty/sidecar.client';

interface OkResponse {
  ok: boolean;
  error?: { code: string; message: string };
}

export class AgentError extends Error {
  code?: string;
  constructor(message: string, code?: string) {
    super(message);
    this.name = 'AgentError';
    this.code = code;
  }
}

const post = async <T>(command: string, body: unknown): Promise<T> => {
  let res: Response;
  try {
    res = await fetch(`${SIDECAR_HTTP}/agents/${command}`, { method: 'POST', body: JSON.stringify(body) });
  } catch {
    throw new AgentError('Brain unavailable');
  }
  if (!res.ok) throw new AgentError('Brain unavailable');
  let json: OkResponse & T;
  try {
    json = (await res.json()) as OkResponse & T;
  } catch {
    throw new AgentError('Bad response');
  }
  if (!json.ok) throw new AgentError(json.error?.message ?? command, json.error?.code);
  return json;
};

export const listAgents = (project?: string): Promise<{ agents: PersistentAgent[] }> =>
  post('list', project != null ? { project } : {});

export const showAgent = (id: string): Promise<{ agent: PersistentAgent }> =>
  post('show', { id });

export interface CreateAgentOpts {
  name: string;
  title?: string;
  description?: string;
  profile: string;
  model?: string;
  effort?: string;
  cwd: string;
  instructions?: string;
  autoStart?: boolean;
}

export const createAgent = (opts: CreateAgentOpts): Promise<{ agent: PersistentAgent }> =>
  post('create', opts);

export interface UpdateAgentOpts {
  id: string;
  name?: string;
  title?: string;
  description?: string;
  profile?: string;
  model?: string | null;
  effort?: string | null;
  cwd?: string;
  instructions?: string;
  autoStart?: boolean;
}

export const updateAgent = (opts: UpdateAgentOpts): Promise<{ agent: PersistentAgent; restartNeeded: boolean }> =>
  post('update', opts);

export const deleteAgent = (id: string): Promise<void> =>
  post<Record<string, never>>('delete', { id }).then(() => undefined);

export const startAgent = (id: string, fresh?: boolean): Promise<{ agent: PersistentAgent; live: boolean }> =>
  post('start', fresh === true ? { id, fresh: true } : { id });

export const stopAgent = (id: string, force?: boolean): Promise<{ agent: PersistentAgent }> =>
  post('stop', force === true ? { id, force: true } : { id });
