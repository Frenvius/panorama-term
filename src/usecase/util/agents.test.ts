import { describe, it, expect } from 'bun:test';

import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { PersistentAgent } from '~/domain/interfaces/agents.interface';

import { sortAgents, statusToDotKind, isValidAgentName, utf8ByteLength, isHeadlessTile, isNotResumable, needsForceStop } from '~/usecase/util/agents';

const baseAgent = (overrides: Partial<PersistentAgent> = {}): PersistentAgent => ({
  id: 'a0000001',
  name: 'test',
  title: 'Test',
  description: '',
  profile: 'claude',
  model: null,
  effort: null,
  cwd: '/tmp',
  project: null,
  autoStart: true,
  desired: 'stopped',
  nativeSession: null,
  lastExit: null,
  crashed: false,
  createdAt: 0,
  updatedAt: 0,
  lastStartedAt: null,
  tileId: 'agent-a0000001',
  status: 'stopped',
  live: false,
  statusSource: 'none',
  ...overrides,
});

describe('sortAgents', () => {
  it('puts live agents before stopped ones', () => {
    const a = baseAgent({ name: 'z', live: false, status: 'stopped' });
    const b = baseAgent({ name: 'a', live: true, status: 'idle' });
    const sorted = sortAgents([a, b]);
    expect(sorted[0].name).toBe('a');
  });

  it('sorts by name within the same live group', () => {
    const a = baseAgent({ name: 'bravo', live: true, status: 'idle' });
    const b = baseAgent({ name: 'alpha', live: true, status: 'idle' });
    const sorted = sortAgents([a, b]);
    expect(sorted[0].name).toBe('alpha');
  });
});

describe('statusToDotKind', () => {
  it('maps idle to idle', () => expect(statusToDotKind('idle')).toBe('idle'));
  it('maps busy to busy', () => expect(statusToDotKind('busy')).toBe('busy'));
  it('maps starting to busy', () => expect(statusToDotKind('starting')).toBe('busy'));
  it('maps waiting to alert', () => expect(statusToDotKind('waiting')).toBe('alert'));
  it('maps crashed to alert', () => expect(statusToDotKind('crashed')).toBe('alert'));
  it('maps stopped to detached', () => expect(statusToDotKind('stopped')).toBe('detached'));
});

describe('isValidAgentName', () => {
  it('accepts valid names', () => {
    expect(isValidAgentName('reviewer')).toBe(true);
    expect(isValidAgentName('my-agent-01')).toBe(true);
    expect(isValidAgentName('a')).toBe(true);
  });
  it('rejects names starting with dash', () => expect(isValidAgentName('-bad')).toBe(false));
  it('rejects uppercase', () => expect(isValidAgentName('Bad')).toBe(false));
  it('rejects empty', () => expect(isValidAgentName('')).toBe(false));
  it('rejects too long (33 chars)', () => expect(isValidAgentName('a'.repeat(33))).toBe(false));
  it('accepts 32 chars', () => expect(isValidAgentName('a'.repeat(32))).toBe(true));
});

describe('utf8ByteLength', () => {
  it('counts ASCII correctly', () => expect(utf8ByteLength('hello')).toBe(5));
  it('counts multi-byte correctly', () => expect(utf8ByteLength('\u00e9')).toBe(2));
  it('counts emoji as 4 bytes', () => expect(utf8ByteLength('\u{1F600}')).toBe(4));
});

describe('isHeadlessTile', () => {
  const baseTile: Tile = { id: 't1', type: 'term', x: 0, y: 0, width: 400, height: 300, zIndex: 1 };
  it('returns false for plain tile', () => expect(isHeadlessTile(baseTile)).toBe(false));
  it('returns true for orchWorker tile', () => expect(isHeadlessTile({ ...baseTile, orchWorker: true })).toBe(true));
  it('returns true for agentId tile', () => expect(isHeadlessTile({ ...baseTile, agentId: 'a001' })).toBe(true));
});

describe('isNotResumable', () => {
  it('flags kimi and opencode agents without a captured session, live or not', () => {
    expect(isNotResumable(baseAgent({ profile: 'kimi' }))).toBe(true);
    expect(isNotResumable(baseAgent({ profile: 'opencode', live: true, status: 'idle' }))).toBe(true);
  });

  it('clears once a session is captured and never flags hook CLIs', () => {
    const session = { agent: 'kimi', id: 'session_1', boundAt: 1, model: null };
    expect(isNotResumable(baseAgent({ profile: 'kimi', nativeSession: session }))).toBe(false);
    expect(isNotResumable(baseAgent({ profile: 'claude' }))).toBe(false);
  });
});

describe('needsForceStop', () => {
  it('requires force while busy, waiting or when the live status is unknown', () => {
    expect(needsForceStop(baseAgent({ live: true, status: 'busy', statusSource: 'hook' }))).toBe(true);
    expect(needsForceStop(baseAgent({ live: true, status: 'waiting', statusSource: 'hook' }))).toBe(true);
    expect(needsForceStop(baseAgent({ live: true, status: 'idle', statusSource: 'none' }))).toBe(true);
  });

  it('allows a plain stop when the status is known idle or the agent is not live', () => {
    expect(needsForceStop(baseAgent({ live: true, status: 'idle', statusSource: 'hook' }))).toBe(false);
    expect(needsForceStop(baseAgent({ live: true, status: 'idle', statusSource: 'quiet' }))).toBe(false);
    expect(needsForceStop(baseAgent({ live: false, status: 'stopped' }))).toBe(false);
  });
});
