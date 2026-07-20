import { expect, test } from 'bun:test';

import { commitUrl } from '~/usecase/util/gitRemote';

const HASH = '7aaa854';

test('builds urls from ssh and https remotes alike', () => {
  const want = `https://github.com/Frenvius/panorama/commit/${HASH}`;

  expect(commitUrl('git@github.com:Frenvius/panorama.git', HASH)).toBe(want);
  expect(commitUrl('https://github.com/Frenvius/panorama.git', HASH)).toBe(want);
  expect(commitUrl('https://github.com/Frenvius/panorama', HASH)).toBe(want);
  expect(commitUrl('ssh://git@github.com/Frenvius/panorama.git', HASH)).toBe(want);
});

test('hosts that use a different commit path', () => {
  expect(commitUrl('git@gitlab.com:team/app.git', HASH)).toBe(`https://gitlab.com/team/app/-/commit/${HASH}`);
  expect(commitUrl('git@bitbucket.org:team/app.git', HASH)).toBe(`https://bitbucket.org/team/app/commits/${HASH}`);
});

test('a hostile remote never escapes into the opened url', () => {
  expect(commitUrl('javascript:alert(1)//github.com/a/b', HASH)).toBeNull();
  expect(commitUrl('file:///etc/passwd', HASH)).toBeNull();
  expect(commitUrl('', HASH)).toBeNull();
  expect(commitUrl('git@github.com:a/b.git', 'not-a-hash')).toBeNull();
  expect(commitUrl('git@github.com:a/b.git', '../../evil')).toBeNull();
});
