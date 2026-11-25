import { describe, it, expect } from 'bun:test';

import type { AgentMessage, Party } from '~/domain/interfaces/agents.interface';

import { messageDirection, messagePreview, statusChipKind, countUnread, clampToViewport } from '~/usecase/util/agentMessages';

const userParty: Party = { kind: 'user' };
const agentParty = (id: string): Party => ({ kind: 'agent', id });

const baseMsg = (overrides: Partial<AgentMessage> = {}): AgentMessage => ({
  id: 'm000001',
  from: agentParty('a001'),
  to: agentParty('a002'),
  fromName: 'planner',
  fromLabel: 'Planner',
  toLabel: 'Reviewer',
  text: 'hello',
  expectsReply: false,
  replyTo: null,
  thread: 'm000001',
  hops: 0,
  at: 1700000000000,
  seq: 1,
  status: 'delivered',
  deliveredAt: 1700000001000,
  repliedBy: null,
  reason: null,
  read: false,
  notice: false,
  ...overrides,
});

describe('messageDirection', () => {
  it('in when to is this agent', () => {
    expect(messageDirection(baseMsg({ to: agentParty('a002') }), 'a002')).toBe('in');
  });

  it('out when from is this agent', () => {
    expect(messageDirection(baseMsg({ from: agentParty('a001'), to: agentParty('a002') }), 'a001')).toBe('out');
  });

  it('in for user when to is user', () => {
    expect(messageDirection(baseMsg({ to: userParty }), 'user')).toBe('in');
  });

  it('out for user when to is not user', () => {
    expect(messageDirection(baseMsg({ from: userParty, to: agentParty('a001') }), 'user')).toBe('out');
  });
});

describe('statusChipKind', () => {
  it('returns message status', () => {
    expect(statusChipKind(baseMsg({ status: 'queued' }))).toBe('queued');
    expect(statusChipKind(baseMsg({ status: 'delivered' }))).toBe('delivered');
    expect(statusChipKind(baseMsg({ status: 'dropped' }))).toBe('dropped');
    expect(statusChipKind(baseMsg({ status: 'failed' }))).toBe('failed');
  });
});

describe('messagePreview', () => {
  it('replaces newlines with slash', () => {
    expect(messagePreview(baseMsg({ text: 'line1\nline2' }))).toBe('line1 / line2');
  });

  it('truncates at 80 chars with ellipsis', () => {
    const preview = messagePreview(baseMsg({ text: 'a'.repeat(100) }));
    expect(preview.endsWith('...')).toBe(true);
    expect(preview.length).toBe(82);
  });

  it('keeps short text unchanged', () => {
    expect(messagePreview(baseMsg({ text: 'short text' }))).toBe('short text');
  });
});

describe('countUnread', () => {
  it('counts unread messages to user only', () => {
    const msgs = [
      baseMsg({ to: userParty, read: false }),
      baseMsg({ to: userParty, read: true }),
      baseMsg({ to: agentParty('a001'), read: false }),
    ];
    expect(countUnread(msgs)).toBe(1);
  });

  it('returns 0 when all read', () => {
    expect(countUnread([baseMsg({ to: userParty, read: true })])).toBe(0);
  });

  it('returns 0 for empty list', () => {
    expect(countUnread([])).toBe(0);
  });
});

describe('clampToViewport', () => {
  it('keeps a popover inside the window', () => {
    const viewport = { width: 1000, height: 600 };
    const size = { width: 360, height: 480 };
    expect(clampToViewport(900, 500, size, viewport)).toEqual({ left: 632, top: 112 });
    expect(clampToViewport(100, 50, size, viewport)).toEqual({ left: 100, top: 50 });
    expect(clampToViewport(-20, -5, size, viewport)).toEqual({ left: 8, top: 8 });
  });
});
