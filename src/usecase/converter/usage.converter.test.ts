import { expect, test } from 'bun:test';

import { antigravityUsageConverter } from '~/usecase/converter/usage.converter';

const data = {
  groups: [
    {
      name: 'Gemini Models',
      buckets: [
        { id: 'gemini-weekly', window: 'weekly', remaining_fraction: 0.93, reset_time: '2026-11-29T01:35:13Z' },
        { id: 'gemini-5h', window: '5h', remaining_fraction: 0.79, reset_time: '2026-11-22T06:35:13Z' }
      ]
    },
    {
      name: 'Claude and GPT models',
      buckets: [
        { id: '3p-weekly', window: 'weekly', remaining_fraction: 1, reset_time: '1970-01-01T00:00:00Z' },
        { id: '3p-5h', window: '5h', remaining_fraction: 0, disabled: true, reset_time: '1970-01-01T00:00:00Z' }
      ]
    },
    {
      name: 'Gemini 4 Argon',
      buckets: [{ id: 'argon-weekly', window: 'weekly', remaining_fraction: 0.5, reset_time: '2026-11-29T05:17:20Z' }]
    }
  ]
};

test('antigravity keeps the tightest 5h and weekly pool', () => {
  const [session, weekly] = antigravityUsageConverter(data);
  expect(session).toEqual({
    used: 21,
    key: 'session',
    label: '5h',
    title: 'Session · Gemini',
    resetsAt: Date.parse('2026-11-22T06:35:13Z')
  });
  expect(weekly?.used).toBe(50);
  expect(weekly?.title).toBe('Weekly · Gemini 4 Argon');
});

test('antigravity drops epoch resets and tolerates empty data', () => {
  const [, weekly] = antigravityUsageConverter({ groups: [data.groups[0], { name: 'X', buckets: [{ window: 'weekly', remaining_fraction: 0, reset_time: '1970-01-01T00:00:00Z' }] }] });
  expect(weekly?.resetsAt).toBeNull();
  expect(antigravityUsageConverter({})).toEqual([]);
});
