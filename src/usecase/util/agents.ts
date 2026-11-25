import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { AgentStatus, PersistentAgent } from '~/domain/interfaces/agents.interface';

export type DotKind = 'idle' | 'busy' | 'alert' | 'detached';

export const statusToDotKind = (status: AgentStatus): DotKind => {
  if (status === 'idle') return 'idle';
  if (status === 'busy' || status === 'starting') return 'busy';
  if (status === 'waiting' || status === 'crashed') return 'alert';
  return 'detached';
};

const NAME_RE = /^[a-z0-9][a-z0-9-]{0,31}$/;

export const isValidAgentName = (name: string): boolean => NAME_RE.test(name);

export const utf8ByteLength = (s: string): number => new TextEncoder().encode(s).length;

export const sortAgents = (agents: PersistentAgent[]): PersistentAgent[] =>
  [...agents].sort((a, b) => {
    const aLive = a.live ? 0 : 1;
    const bLive = b.live ? 0 : 1;
    if (aLive !== bLive) return aLive - bLive;
    return a.name.localeCompare(b.name);
  });

export const isHeadlessTile = (t: Tile): boolean => Boolean(t.orchWorker) || Boolean(t.agentId);

export const isNotResumable = (agent: PersistentAgent): boolean =>
  agent.nativeSession === null &&
  (agent.profile === 'kimi' || agent.profile === 'opencode');
