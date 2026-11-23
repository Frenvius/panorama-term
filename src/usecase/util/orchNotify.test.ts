import { expect, test } from 'bun:test';

import { orchNotificationText } from '~/usecase/util/orchNotify';

test('formats each board notification', () => {
  const base = { runId: 'r', runTitle: 'Board', taskTitle: 'Task', detail: 'file' };
  expect(orchNotificationText({ ...base, kind: 'gate' })).toBe('Gate waiting: file');
  expect(orchNotificationText({ ...base, kind: 'review' })).toBe('Task ready for review: Task');
  expect(orchNotificationText({ ...base, kind: 'exhausted' })).toBe('Task failed after all attempts: Task');
  expect(orchNotificationText({ ...base, kind: 'conflict' })).toBe('Merge conflict in Task: file');
  expect(orchNotificationText({ ...base, kind: 'completed' })).toBe('Board finished');
});
