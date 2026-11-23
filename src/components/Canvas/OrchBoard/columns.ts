import type { Task, Dispatch } from '~/domain/interfaces/orch.interface';

export type ColumnId = 'todo' | 'running' | 'review' | 'done';

export const COLUMN_LABEL: Record<ColumnId, string> = {
  todo: 'To do',
  running: 'Running',
  review: 'Review',
  done: 'Done'
};

export const COLUMNS: ColumnId[] = ['todo', 'running', 'review', 'done'];

export const taskColumn = (task: Task): ColumnId => {
  if (task.status === 'pending') return 'todo';
  if (task.status === 'dispatched' || task.status === 'verifying') return 'running';
  if (task.status === 'waiting') return task.wait?.kind === 'intervention' ? 'review' : 'running';
  if (task.status === 'review' || task.status === 'failed' || task.status === 'blocked') return 'review';
  return 'done';
};

export const latestDispatch = (task: Task, dispatches: Dispatch[]): Dispatch | undefined =>
  dispatches.filter((d) => d.taskId === task.id).sort((a, b) => b.attempt - a.attempt)[0];
