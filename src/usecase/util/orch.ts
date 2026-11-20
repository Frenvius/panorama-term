import type { Task, Dispatch } from '~/domain/interfaces/orch.interface';

export const ACTIVE_DISPATCH_STATUSES = new Set(['starting', 'running', 'reported']);

export const topoSort = (tasks: Task[]): Task[] => {
  const byId = new Map(tasks.map((t) => [t.id, t]));
  const result: Task[] = [];
  const visited = new Set<string>();
  const visit = (id: string): void => {
    if (visited.has(id)) return;
    visited.add(id);
    const task = byId.get(id);
    if (!task) return;
    for (const dep of task.deps) visit(dep);
    result.push(task);
  };
  for (const task of tasks) visit(task.id);
  return result;
};

export const formatDuration = (startedAt: number, endedAt?: number): string => {
  const ms = (endedAt ?? Date.now()) - startedAt;
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${s % 60}s`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
};

export const totalCost = (dispatches: Dispatch[]): number | null => {
  const costs = dispatches.filter((d) => d.costUsd != null).map((d) => d.costUsd!);
  return costs.length > 0 ? costs.reduce((a, b) => a + b, 0) : null;
};

export const taskCountSummary = (counts: Record<string, number>): string => {
  const done = (counts['completed'] ?? 0) + (counts['cancelled'] ?? 0);
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  return `${done}/${total}`;
};
