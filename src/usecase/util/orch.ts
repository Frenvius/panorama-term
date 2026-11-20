import type { Role, Task, Dispatch } from '~/domain/interfaces/orch.interface';

export const ACTIVE_DISPATCH_STATUSES = new Set(['starting', 'running', 'reported']);

export const ROLES: Role[] = ['explore', 'implement', 'test', 'review', 'mechanical'];

export const ROLE_META: Record<Role, { label: string; description: string }> = {
  explore: { label: 'Explore', description: 'Read, search, and map code; no edits.' },
  implement: { label: 'Implement', description: 'Write or change code to a spec.' },
  test: { label: 'Test', description: 'Write and run tests for finished work.' },
  review: { label: 'Review', description: 'Read a diff and report problems; no edits.' },
  mechanical: { label: 'Mechanical', description: 'Renames, formatting, boilerplate, moves.' }
};

export const normalizeRepo = (p: string): string => p.trim().replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();

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
