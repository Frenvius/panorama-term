import { expect, test } from 'bun:test';

import { parseManifest, readPlugins, toggleDisabled, enabledManifests } from '~/usecase/util/plugins';

const manifest = (over: Record<string, unknown> = {}): string =>
  JSON.stringify({ id: 'nord', name: 'Nord', version: '1.0.0', ...over });

test('a valid manifest keeps its fields', () => {
  expect(parseManifest('nord', manifest({ description: 'Cold colors', author: 'me' }))).toEqual({
    id: 'nord',
    name: 'Nord',
    version: '1.0.0',
    description: 'Cold colors',
    author: 'me',
    contributes: { themes: [], fonts: [] }
  });
});

test('pre-release versions are accepted', () => {
  expect(parseManifest('nord', manifest({ version: '2.1.0-beta.1' }))).toMatchObject({ version: '2.1.0-beta.1' });
});

test('broken manifests explain what is wrong', () => {
  expect(parseManifest('nord', null)).toBe('Missing plugin.json');
  expect(parseManifest('nord', '{oops')).toBe('plugin.json is not valid JSON');
  expect(parseManifest('nord', '[]')).toBe('plugin.json must be a JSON object');
  expect(parseManifest('nord', manifest({ id: 'Nord Theme' }))).toBe('"id" must use lowercase letters, digits and dashes');
  expect(parseManifest('other', manifest())).toBe('"id" must match the folder name "other"');
  expect(parseManifest('nord', manifest({ name: ' ' }))).toBe('"name" is required');
  expect(parseManifest('nord', manifest({ version: 'v1' }))).toBe('"version" must look like 1.0.0');
  expect(parseManifest('nord', manifest({ description: 3 }))).toBe('"description" must be text');
  expect(parseManifest('nord', manifest({ author: {} }))).toBe('"author" must be text');
});

test('plugins are enabled unless disabled, and broken ones carry their error', () => {
  const plugins = readPlugins(
    [
      { dir: 'nord', manifest: manifest() },
      { dir: 'mono', manifest: manifest({ id: 'mono' }) },
      { dir: 'empty', manifest: null }
    ],
    ['mono']
  );
  expect(plugins.map((p) => ('error' in p ? p.error : p.enabled))).toEqual([true, false, 'Missing plugin.json']);
  expect(enabledManifests(plugins).map((m) => m.id)).toEqual(['nord']);
});

test('themes come from contributes, and a bad theme breaks the manifest', () => {
  const theme = { id: 'night', label: 'Nord Night', kind: 'dark', colors: { accent: '#88c0d0' } };
  expect(parseManifest('nord', manifest({ contributes: { themes: [theme] } }))).toMatchObject({
    contributes: { themes: [{ ...theme, terminal: {} }] }
  });
  expect(parseManifest('nord', manifest({ contributes: [] }))).toBe('"contributes" must be an object');
  expect(parseManifest('nord', manifest({ contributes: { themes: [{ ...theme, kind: 'dim' }] } }))).toBe(
    'themes[0] "kind" must be "dark" or "light"'
  );
});

test('toggling flips a plugin in the disabled list', () => {
  expect(toggleDisabled([], 'nord')).toEqual(['nord']);
  expect(toggleDisabled(['mono', 'nord'], 'nord')).toEqual(['mono']);
});
