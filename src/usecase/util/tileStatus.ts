import type { NotifyKind } from '~/domain/interfaces/notify.interface';

export type TileStatus = 'waiting' | 'working' | 'done';

const WAITING: NotifyKind[] = ['permission', 'attention', 'idle'];

export const tileStatus = (alert?: NotifyKind, agent?: 'idle' | 'busy'): TileStatus | null => {
  if (alert && WAITING.includes(alert)) return 'waiting';
  if (agent === 'busy') return 'working';
  return alert ? 'done' : null;
};

export const elapsed = (since: number, now: number): string => {
  const mins = Math.floor((now - since) / 60000);
  if (mins < 1) return '<1m';
  if (mins < 60) return `${mins}m`;
  return `${Math.floor(mins / 60)}h${mins % 60 ? `${mins % 60}m` : ''}`;
};
