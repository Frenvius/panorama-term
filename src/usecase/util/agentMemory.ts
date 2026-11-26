import type { AgentMemory, MemoryUsage, AgentMessage, PersistentAgent } from '~/domain/interfaces/agents.interface';

export const DEFAULT_ROLLOVER_AT = 70;
export const ROLLOVER_AT_MIN = 40;
export const ROLLOVER_AT_MAX = 90;

const CONTEXT_PROFILES = ['claude', 'kimi'];

export const reportsContext = (profileId: string): boolean => CONTEXT_PROFILES.includes(profileId);

export interface UsageBar {
  pct: number;
  bytesText: string;
  entriesText: string;
  full: boolean;
}

const thousands = (n: number): string => n.toLocaleString('en-US');

export const usageBar = (u: MemoryUsage): UsageBar => ({
  pct: u.maxBytes > 0 ? Math.min(100, Math.round((u.essentialBytes / u.maxBytes) * 100)) : 0,
  bytesText: `${thousands(u.essentialBytes)} / ${thousands(u.maxBytes)} bytes`,
  entriesText: `${u.essentialCount} / ${u.maxEntries} essential`,
  full: u.essentialBytes >= u.maxBytes || u.essentialCount >= u.maxEntries
});

const utf8 = (text: string): number => new TextEncoder().encode(text).length;

export const canMakeEssential = (m: AgentMemory, u: MemoryUsage): boolean =>
  m.essential || (u.essentialBytes + utf8(m.text) <= u.maxBytes && u.essentialCount + 1 <= u.maxEntries);

export const sortMemories = (list: AgentMemory[]): AgentMemory[] =>
  [...list].sort((a, b) => Number(b.essential) - Number(a.essential) || b.updatedAt - a.updatedAt);

export const renewDisabledReason = (agent: PersistentAgent): string | null => {
  if (agent.rollover?.open) return 'A session renewal is in progress';
  if (!agent.live) return 'The agent is not running';
  if (agent.status === 'busy' || agent.status === 'waiting') return 'The agent is busy; renew when it is idle';
  return null;
};

export const isRenewing = (agent: PersistentAgent): boolean => Boolean(agent.rollover?.open);

export const rolloverAtLabel = (at: number | undefined): string => (at === 0 ? 'off' : `${at ?? DEFAULT_ROLLOVER_AT}%`);

export const parseRolloverAt = (raw: string): number | null => {
  const text = raw.trim().toLowerCase();
  if (text === 'off') return 0;
  const n = Number(text.replace('%', ''));
  return Number.isInteger(n) && n >= ROLLOVER_AT_MIN && n <= ROLLOVER_AT_MAX ? n : null;
};

export const renewedText = (before: number | undefined, after: number | undefined): string =>
  before != null && after != null ? `session renewed (${before}% -> ${after}%)` : after != null ? `session renewed (now ${after}%)` : 'session renewed';

export const isRolloverMessage = (msg: AgentMessage): boolean => msg.kind === 'rollover';