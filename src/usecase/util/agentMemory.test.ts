import { describe, it, expect } from 'bun:test';
import type { AgentMemory, MemoryUsage, PersistentAgent } from '~/domain/interfaces/agents.interface';
import { usageBar, renewedText, isRenewing, parseRolloverAt, sortMemories, reportsContext, canMakeEssential, renewDisabledReason } from '~/usecase/util/agentMemory';

const usage = (bytes: number, count: number): MemoryUsage => ({ count: 20, essentialBytes: bytes, essentialCount: count, maxBytes: 2048, maxEntries: 12, max: 64 });
const mem = (id: string, essential: boolean, text = 'x', updatedAt = 1): AgentMemory => ({ id, text, essential, by: 'agent', createdAt: 1, updatedAt });
const agent = (patch: Partial<PersistentAgent>): PersistentAgent => ({ live: true, status: 'idle', ...patch } as PersistentAgent);

describe('memory usage', () => {
  it('computes the bar and the full state', () => {
    expect(usageBar(usage(1024, 3))).toEqual({ pct: 50, bytesText: '1,024 / 2,048 bytes', entriesText: '3 / 12 essential', full: false });
    expect(usageBar(usage(2048, 3)).full).toBe(true);
    expect(usageBar(usage(10, 12)).full).toBe(true);
    expect(usageBar(usage(4096, 1)).pct).toBe(100);
  });

  it('disables the essential toggle at the cap but always allows demoting', () => {
    expect(canMakeEssential(mem('a', false, 'x'.repeat(100)), usage(1948, 3))).toBe(true);
    expect(canMakeEssential(mem('a', false, 'x'.repeat(101)), usage(1948, 3))).toBe(false);
    expect(canMakeEssential(mem('a', false), usage(10, 12))).toBe(false);
    expect(canMakeEssential(mem('a', true), usage(2048, 12))).toBe(true);
  });

  it('lists essential first, newest first', () => {
    expect(sortMemories([mem('a', false, 'x', 5), mem('b', true, 'x', 1), mem('c', true, 'x', 9)]).map((m) => m.id)).toEqual(['c', 'b', 'a']);
  });
});

describe('renew session', () => {
  it('is disabled while busy, renewing or stopped', () => {
    expect(renewDisabledReason(agent({}))).toBeNull();
    expect(renewDisabledReason(agent({ status: 'busy' }))).toContain('busy');
    expect(renewDisabledReason(agent({ live: false, status: 'stopped' }))).toContain('not running');
    const rolling = agent({ rollover: { open: { n: 1, reason: 'user', phase: 'open', startedAt: 1 } } });
    expect(renewDisabledReason(rolling)).toContain('in progress');
    expect(isRenewing(rolling)).toBe(true);
  });

  it('shows the threshold field only for CLIs that report context', () => {
    expect(reportsContext('claude')).toBe(true);
    expect(reportsContext('kimi')).toBe(true);
    expect(reportsContext('codex')).toBe(false);
    expect(parseRolloverAt('off')).toBe(0);
    expect(parseRolloverAt('55%')).toBe(55);
    expect(parseRolloverAt('95')).toBeNull();
    expect(parseRolloverAt('39')).toBeNull();
  });

  it('writes the renewed text for the rollover chip', () => {
    expect(renewedText(72, 9)).toBe('session renewed (72% -> 9%)');
    expect(renewedText(undefined, 9)).toBe('session renewed (now 9%)');
    expect(renewedText(undefined, undefined)).toBe('session renewed');
  });
});