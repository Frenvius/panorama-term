import { expect, test } from 'bun:test';

import { notePreview } from '~/usecase/util/noteMeta';
import { elapsed, tileStatus } from '~/usecase/util/tileStatus';

test('waiting beats working, unread finish is done', () => {
  expect(tileStatus('permission', 'busy')).toBe('waiting');
  expect(tileStatus(undefined, 'busy')).toBe('working');
  expect(tileStatus('finished', 'idle')).toBe('done');
  expect(tileStatus(undefined, 'idle')).toBeNull();
});

test('elapsed formats minutes and hours', () => {
  expect(elapsed(0, 30000)).toBe('<1m');
  expect(elapsed(0, 12 * 60000)).toBe('12m');
  expect(elapsed(0, 125 * 60000)).toBe('2h5m');
  expect(elapsed(0, 120 * 60000)).toBe('2h');
});

test('note preview skips frontmatter and markdown markers', () => {
  const content = '---\ntitle: Sprint\n---\n# Sprint 14\n- [x] dropdown\n- [ ] velocity\n- export';
  expect(notePreview(content)).toBe('Sprint 14 · dropdown · velocity');
});
