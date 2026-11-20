import type { Task, Dispatch } from '~/domain/interfaces/orch.interface';

export type ColumnId = 'needsYou' | 'queued' | 'running' | 'verifying' | 'review' | 'done';

export const COLUMN_LABEL: Record<ColumnId, string> = {
  needsYou: 'Needs you',
  queued: 'Queued',
  running: 'Running',
  verifying: 'Verifying',
  review: 'Review',
  done: 'Done'
};

export const COLUMNS: ColumnId[] = ['needsYou', 'queued', 'running', 'verifying', 'review', 'done'];

export const taskColumn = (task: Task): ColumnId => {
  if (task.status === 'failed' || task.status === 'blocked') return 'needsYou';
  if (task.status === 'pending') return 'queued';
  if (task.status === 'dispatched') return 'running';
  if (task.status === 'verifying') return 'verifying';
  if (task.status === 'review') return 'review';
  return 'done';
};

export const latestDispatch = (task: Task, dispatches: Dispatch[]): Dispatch | undefined =>
  dispatches.filter((d) => d.taskId === task.id).sort((a, b) => b.attempt - a.attempt)[0];
