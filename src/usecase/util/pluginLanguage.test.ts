import { expect, test } from 'bun:test';

import { findLanguage, parseLanguages } from '~/usecase/util/pluginLanguage';

test('a language has a grammar, extensions, or both', () => {
  expect(parseLanguages([{ id: 'demo', extensions: ['.DMO'], grammar: 'syntaxes/demo.json' }])).toEqual([
    { id: 'demo', extensions: ['.dmo'], grammar: 'syntaxes/demo.json' }
  ]);
  expect(parseLanguages([{ id: 'groovy', extensions: ['.jenkins'] }])).toEqual([
    { id: 'groovy', extensions: ['.jenkins'], grammar: undefined }
  ]);
  expect(parseLanguages(undefined)).toEqual([]);
});

test('bad languages explain what is wrong', () => {
  expect(parseLanguages({})).toBe('"languages" must be a list');
  expect(parseLanguages(['x'])).toBe('languages[0] must be an object');
  expect(parseLanguages([{ id: 'Demo', grammar: 'a.json' }])).toBe(
    'languages[0] "id" must use lowercase letters, digits and + # . -'
  );
  expect(parseLanguages([{ id: 'demo', extensions: ['dmo'] }])).toBe('languages[0] "extensions" must list extensions like ".ext"');
  expect(parseLanguages([{ id: 'demo', grammar: '../a.json' }])).toBe('languages[0] "grammar" must be a .json file inside the plugin');
  expect(parseLanguages([{ id: 'demo', grammar: 'a.xml' }])).toBe('languages[0] "grammar" must be a .json file inside the plugin');
  expect(parseLanguages([{ id: 'demo' }])).toBe('languages[0] needs a "grammar" or "extensions"');
  expect(parseLanguages([{ id: 'demo', grammar: 'a.json' }, { id: 'demo', grammar: 'b.json' }])).toBe(
    'languages[1] repeats the id "demo"'
  );
});

test('a file extension or a grammar id finds the plugin language', () => {
  const plugins = [
    {
      id: 'demo',
      contributes: {
        languages: [
          { id: 'demo', extensions: ['.dmo'], grammar: 'demo.json' },
          { id: 'groovy', extensions: ['.jenkins'] }
        ]
      }
    }
  ] as never[];
  expect(findLanguage(plugins, 'DMO')?.language.id).toBe('demo');
  expect(findLanguage(plugins, 'demo')?.language.id).toBe('demo');
  expect(findLanguage(plugins, 'jenkins')?.language.id).toBe('groovy');
  expect(findLanguage(plugins, 'groovy')).toBeNull();
  expect(findLanguage(plugins, 'ts')).toBeNull();
});
