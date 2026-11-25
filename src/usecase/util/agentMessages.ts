import type { AgentMessage, MessageStatus } from '~/domain/interfaces/agents.interface';

export type MessageDirection = 'in' | 'out';

export const messageDirection = (msg: AgentMessage, party: string): MessageDirection => {
  if (party === 'user') return msg.to.kind === 'user' ? 'in' : 'out';
  if (msg.to.kind === 'agent' && msg.to.id === party) return 'in';
  return 'out';
};

export type StatusChipKind = MessageStatus;

export const statusChipKind = (msg: AgentMessage): StatusChipKind => msg.status;

export const messagePreview = (msg: AgentMessage): string => {
  const flat = msg.text.replace(/\n/g, ' / ');
  return flat.length > 80 ? flat.slice(0, 79) + '...' : flat;
};

export const countUnread = (messages: AgentMessage[]): number =>
  messages.filter((m) => !m.read && m.to.kind === 'user').length;

export const clampToViewport = (left: number, top: number, size: { width: number; height: number }, viewport: { width: number; height: number }): { left: number; top: number } => ({
  left: Math.max(8, Math.min(left, viewport.width - size.width - 8)),
  top: Math.max(8, Math.min(top, viewport.height - size.height - 8))
});
