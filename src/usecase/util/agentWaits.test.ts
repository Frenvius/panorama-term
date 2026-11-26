import { describe, it, expect } from 'bun:test';
import type { AgentWait, PersistentAgent } from '~/domain/interfaces/agents.interface';
import { formatLeft, waitTooltip, splitWaits, agentForTile, startedByLabel, coordinatorCandidates } from '~/usecase/util/agentWaits';

const agent = (name: string, canCoordinate: boolean): PersistentAgent =>
  ({ name, canCoordinate, tileId: `agent-${name}` }) as unknown as PersistentAgent;

const wait = (id: string, status: AgentWait['status'], at: number): AgentWait => ({
  id,
  on: ['timer'],
  note: '',
  createdAt: at,
  timeoutAt: null,
  status,
  fired: status === 'fired' ? { at, summary: 'timer.', wake: true, started: false } : null,
  cancelledBy: null
});

describe('formatLeft', () => {
  it('formats minutes and hours', () => {
    expect(formatLeft(0)).toBe('due');
    expect(formatLeft(90_000)).toBe('2m left');
    expect(formatLeft(2 * 3_600_000)).toBe('2h left');
    expect(formatLeft(80 * 60_000)).toBe('1h 20m left');
  });
});

describe('waitTooltip', () => {
  it('lists targets, time left and the note', () => {
    const text = waitTooltip([
      { id: 'w1', on: ['build in D:/x'], note: 'fix build', timeoutAt: 1_000 + 30 * 60_000 },
      { id: 'w2', on: ['timer 2026-11-25T18:00:00Z'], note: '', timeoutAt: null }
    ], 1_000);
    expect(text).toBe('waiting on 2 events\nbuild in D:/x (30m left) - fix build\ntimer 2026-11-25T18:00:00Z');
    expect(waitTooltip([{ id: 'w1', on: ['a', 'b'], note: '', timeoutAt: null }], 0)).toBe('waiting on 1 event\na | b');
  });
});

describe('startedByLabel', () => {
  it('names what started the agent', () => {
    expect(startedByLabel('wait w123')).toBe('started by wait');
    expect(startedByLabel('orch r6un')).toBe('started by board');
    expect(startedByLabel('routine rt3k2a')).toBe('started by routine');
    expect(startedByLabel('message m1')).toBe('started by message');
  });
});

describe('coordinatorCandidates', () => {
  it('keeps only agents that can coordinate, by name', () => {
    const list = coordinatorCandidates([agent('zed', true), agent('kimi', false), agent('alpha', true)]);
    expect(list.map((a) => a.name)).toEqual(['alpha', 'zed']);
  });
});

describe('agentForTile', () => {
  it('finds the agent behind a coordinator tile', () => {
    const agents = [agent('planner', true)];
    expect(agentForTile(agents, 'agent-planner')?.name).toBe('planner');
    expect(agentForTile(agents, 'tile-123')).toBeNull();
    expect(agentForTile(agents, null)).toBeNull();
  });
});

describe('splitWaits', () => {
  it('separates open waits from the last ten closed ones, newest first', () => {
    const closed = Array.from({ length: 12 }, (_, i) => wait(`f${i}`, 'fired', i));
    const { open, history } = splitWaits([wait('o1', 'open', 5), ...closed, wait('c1', 'cancelled', 100)]);
    expect(open.map((w) => w.id)).toEqual(['o1']);
    expect(history).toHaveLength(10);
    expect(history[0].id).toBe('c1');
    expect(history[1].id).toBe('f11');
  });
});
