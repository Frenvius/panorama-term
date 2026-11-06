export type AgentStatus = 'idle' | 'busy';

const listeners = new Set<() => void>();
let statuses = new Map<string, AgentStatus>();

export const toAgentStatus = (raw: string): AgentStatus | null => {
  if (raw === 'exited') return null;
  return raw === 'busy' ? 'busy' : 'idle';
};

export const getAgentStatuses = (): Map<string, AgentStatus> => statuses;

export const subscribeAgentStatuses = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const setAgentStatus = (tileId: string, status: AgentStatus | null): void => {
  if ((statuses.get(tileId) ?? null) === status) return;
  statuses = new Map(statuses);
  if (status) statuses.set(tileId, status);
  else statuses.delete(tileId);
  listeners.forEach((fn) => fn());
};
