import type { AgentWait, PersistentAgent, AgentWaitSummary } from '~/domain/interfaces/agents.interface';

const HISTORY_MAX = 10;

export const formatLeft = (ms: number): string => {
  if (ms <= 0) return 'due';
  const minutes = Math.ceil(ms / 60_000);
  if (minutes < 60) return `${minutes}m left`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest ? `${hours}h ${rest}m left` : `${hours}h left`;
};

export const waitTooltip = (waits: AgentWaitSummary[], now: number): string => {
  const head = `waiting on ${waits.length} ${waits.length === 1 ? 'event' : 'events'}`;
  const lines = waits.map((w) => {
    const left = w.timeoutAt != null ? ` (${formatLeft(w.timeoutAt - now)})` : '';
    const note = w.note ? ` - ${w.note}` : '';
    return `${w.on.join(' | ')}${left}${note}`;
  });
  return [head, ...lines].join('\n');
};

export const startedByLabel = (startedBy: string): string => {
  if (startedBy.startsWith('wait ')) return 'started by wait';
  if (startedBy.startsWith('orch ')) return 'started by board';
  if (startedBy.startsWith('routine ')) return 'started by routine';
  return 'started by message';
};

export const coordinatorCandidates = (agents: PersistentAgent[]): PersistentAgent[] =>
  agents.filter((a) => a.canCoordinate).sort((a, b) => a.name.localeCompare(b.name));

export const agentForTile = (agents: PersistentAgent[], tileId: string | null | undefined): PersistentAgent | null =>
  (tileId ? agents.find((a) => a.tileId === tileId) ?? null : null);

export const splitWaits = (waits: AgentWait[]): { open: AgentWait[]; history: AgentWait[] } => {
  const open = waits.filter((w) => w.status === 'open');
  const history = waits
    .filter((w) => w.status !== 'open')
    .sort((a, b) => (b.fired?.at ?? b.createdAt) - (a.fired?.at ?? a.createdAt))
    .slice(0, HISTORY_MAX);
  return { open, history };
};
