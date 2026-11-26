import type { Routine, AgentWait, RoutineRun, RoutineSkip, AgentMemory, MemoryUsage, AgentMessage, MessageStatus, PersistentAgent } from '~/domain/interfaces/agents.interface';

import { getUserCap } from '~/adapter/ui/ui.client';
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
  rolloverAt?: number;
}

export const updateAgent = (opts: UpdateAgentOpts): Promise<{ agent: PersistentAgent; restartNeeded: boolean }> =>
  post('update', { ...opts, userCap: getUserCap() });

export const deleteAgent = (id: string): Promise<void> =>
  post<Record<string, never>>('delete', { id }).then(() => undefined);

export const startAgent = (id: string, fresh?: boolean): Promise<{ agent: PersistentAgent; live: boolean }> =>
  post('start', fresh === true ? { id, fresh: true } : { id });

export const stopAgent = (id: string, force?: boolean): Promise<{ agent: PersistentAgent }> =>
  post('stop', force === true ? { id, force: true } : { id });

export type RecipientStatus =
  | 'running'
  | 'auto_started'
  | 'crashed'
  | 'stopped_loop_guarded'
  | 'stopped_rate_limited';

export interface SendMessageOpts {
  to: string;
  text: string;
  expectsReply?: boolean;
}

export const sendMessage = (
  opts: SendMessageOpts,
): Promise<{ message: AgentMessage; recipientStatus: RecipientStatus; queuedAhead: number; text: string }> =>
  post('send', opts);

export const replyMessage = (
  to: string,
  text: string,
): Promise<{ message: AgentMessage; text: string }> =>
  post('reply', { to, text });

export interface ListMessagesOpts {
  agent?: string;
  party?: 'user';
  thread?: string;
  status?: MessageStatus;
  pending?: boolean;
  limit?: number;
}

export const listMessages = (
  opts: ListMessagesOpts,
): Promise<{ messages: AgentMessage[]; userUnread: number }> =>
  post('messages', opts);

export const readMessage = (id: string): Promise<{ message: AgentMessage }> =>
  post('read', { id });

export const markRead = (ids: string[]): Promise<void> =>
  post<Record<string, never>>('mark-read', { ids }).then(() => undefined);

export const listWaits = (agent: string): Promise<{ waits: AgentWait[] }> =>
  post('waits', { agent });

export const cancelWait = (id: string): Promise<void> =>
  post<{ cancelled: number }>('wait-cancel', { id }).then(() => undefined);

export const coordinateRun = (agent: string, runId: string, on: boolean): Promise<{ coordinates: string[]; text: string }> =>
  post('coordinate', { agent, runId, on });

export const listRoutines = (agent: string): Promise<{ routines: Routine[] }> =>
  post('routines', { agent });

export const createRoutine = (body: Record<string, unknown>): Promise<{ routine: Routine; token?: string }> =>
  post('routine-create', { ...body, userCap: getUserCap() });

export const updateRoutine = (body: Record<string, unknown>): Promise<{ routine: Routine }> =>
  post('routine-update', { ...body, userCap: getUserCap() });

export const setRoutineEnabled = (id: string, on: boolean): Promise<{ routine: Routine }> =>
  post(on ? 'routine-enable' : 'routine-disable', { id, userCap: getUserCap() });

export const deleteRoutine = (id: string): Promise<void> =>
  post<Record<string, never>>('routine-delete', { id, userCap: getUserCap() }).then(() => undefined);

export const runRoutine = (id: string): Promise<{ runId?: string; skipped?: string }> =>
  post('routine-run', { id, userCap: getUserCap() });

export const routineRuns = (id: string, limit: number): Promise<{ runs: RoutineRun[]; skips: RoutineSkip[] }> =>
  post('routine-runs', { id, limit });

export const rotateRoutineToken = (id: string): Promise<{ token: string }> =>
  post('routine-token', { id, rotate: true, userCap: getUserCap() });

export const markAllRead = (uptoSeq: number): Promise<void> =>
  post<Record<string, never>>('mark-read', { all: true, uptoSeq }).then(() => undefined);

export const listMemories = (agent: string): Promise<{ memories: AgentMemory[]; usage: MemoryUsage }> =>
  post('memory', { agent, userCap: getUserCap() });

export const rememberMemory = (agent: string, text: string, essential: boolean): Promise<{ id: string; text: string; usage: MemoryUsage }> =>
  post('memory-remember', { agent, text, essential, userCap: getUserCap() });

export const editMemory = (agent: string, id: string, text: string): Promise<{ text: string; usage: MemoryUsage }> =>
  post('memory-edit', { agent, id, text, userCap: getUserCap() });

export const setMemoryEssential = (agent: string, id: string, essential: boolean): Promise<{ text: string; usage: MemoryUsage }> =>
  post('memory-set', { agent, id, essential, userCap: getUserCap() });

export const forgetMemory = (agent: string, id: string): Promise<{ text: string; usage: MemoryUsage }> =>
  post('memory-forget', { agent, id, userCap: getUserCap() });

export const renewSession = (id: string): Promise<{ n: number; text: string }> =>
  post('rollover', { id, userCap: getUserCap() });