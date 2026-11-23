import type { Task } from '~/domain/interfaces/orch.interface';

import { describe, expect, test } from 'bun:test';

import { conflictFiles, integrationBranchName } from './orch';

const task = (patch: Partial<Task>): Task => ({
  id: 't1',
  title: 'Task',
  spec: '',
  deps: [],
  escalation: [],
  verify: [],
  review: false,
  status: 'pending',
  attempts: 1,
  ready: true,
  createdAt: 0,
  updatedAt: 0,
  ...patch
});

describe('conflictFiles', () => {
  test('lists files of a blocked task', () => {
    expect(conflictFiles(task({ status: 'blocked', result: 'a.rs, src/b.rs' }))).toEqual(['a.rs', 'src/b.rs']);
  });

  test('is empty when blocked without a result', () => {
    expect(conflictFiles(task({ status: 'blocked' }))).toEqual([]);
  });

  test('is empty for other statuses', () => {
    expect(conflictFiles(task({ status: 'completed', result: 'a.rs' }))).toEqual([]);
  });
});

describe('integrationBranchName', () => {
  test('builds the run integration branch', () => {
    expect(integrationBranchName('abc')).toBe('orch/abc/integration');
  });
});
