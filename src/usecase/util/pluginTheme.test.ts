import { expect, test } from 'bun:test';

import { parseThemes } from '~/usecase/util/pluginTheme';

const theme = (over: Record<string, unknown> = {}) => ({ id: 'night', label: 'Night', kind: 'dark', ...over });
const ansi = Array.from({ length: 16 }, (_, i) => `#0000${i.toString(16).padStart(2, '0')}`);

test('a full theme keeps its colors and terminal palette', () => {
  const full = theme({
    colors: { 'surface-1': '#2e3440', 'shiki-token-keyword': '#81a1c1' },
    terminal: { foreground: '#d8dee9', cursor: '#eceff4', ansi }
  });
  expect(parseThemes([full])).toEqual([full as never]);
});

test('colors and terminal are optional', () => {
  expect(parseThemes([theme()])).toEqual([{ ...theme(), colors: {}, terminal: {} } as never]);
  expect(parseThemes(undefined)).toEqual([]);
});

test('bad themes explain what is wrong', () => {
  expect(parseThemes({})).toBe('"themes" must be a list');
  expect(parseThemes(['x'])).toBe('themes[0] must be an object');
  expect(parseThemes([theme({ id: 'Night' })])).toBe('themes[0] "id" must use lowercase letters, digits and dashes');
  expect(parseThemes([theme({ label: '' })])).toBe('themes[0] "label" is required');
  expect(parseThemes([theme({ kind: 'dim' })])).toBe('themes[0] "kind" must be "dark" or "light"');
  expect(parseThemes([theme({ colors: [] })])).toBe('themes[0] "colors" must be an object');
  expect(parseThemes([theme({ colors: { 'font-mono': 'x' } })])).toBe('themes[0] has an unknown color "font-mono"');
  expect(parseThemes([theme({ colors: { accent: 1 } })])).toBe('themes[0] color "accent" must be text');
  expect(parseThemes([theme({ terminal: 'x' })])).toBe('themes[0] "terminal" must be an object');
  expect(parseThemes([theme({ terminal: { foreground: 1 } })])).toBe('themes[0] terminal "foreground" must be text');
  expect(parseThemes([theme({ terminal: { cursor: '' } })])).toBe('themes[0] terminal "cursor" must be text');
  expect(parseThemes([theme({ terminal: { ansi: ansi.slice(1) } })])).toBe('themes[0] terminal "ansi" must list 16 colors');
  expect(parseThemes([theme(), theme()])).toBe('themes[1] repeats the id "night"');
});

test('tile transparency and blur stay within readable limits', () => {
  const ok = theme({ colors: { 'tile-alpha': '45%', 'tile-chrome-alpha': '100%', 'tile-blur': '0px' } });
  expect(parseThemes([ok])).toMatchObject([{ colors: { 'tile-alpha': '45%', 'tile-blur': '0px' } }]);
  expect(parseThemes([theme({ colors: { 'tile-alpha': '10%' } })])).toBe('themes[0] "tile-alpha" must be 30% to 100%');
  expect(parseThemes([theme({ colors: { 'tile-alpha': '0.5' } })])).toBe('themes[0] "tile-alpha" must be 30% to 100%');
  expect(parseThemes([theme({ colors: { 'tile-blur': '40px' } })])).toBe('themes[0] "tile-blur" must be 0px to 32px');
});

test('a background is an image inside the plugin with an optional opacity', () => {
  expect(parseThemes([theme({ background: { image: 'bg/cats.svg' } })])).toMatchObject([
    { background: { image: 'bg/cats.svg', opacity: 1 } }
  ]);
  expect(parseThemes([theme({ background: { image: 'bg/cats.webp', opacity: 0.4 } })])).toMatchObject([
    { background: { image: 'bg/cats.webp', opacity: 0.4 } }
  ]);
  expect(parseThemes([theme({ background: 'cats.svg' })])).toBe('themes[0] "background" must be an object');
  for (const image of ['../cats.svg', '/cats.svg', 'cats.html', 3]) {
    expect(parseThemes([theme({ background: { image } })])).toBe(
      'themes[0] background "image" must be an image file inside the plugin'
    );
  }
  expect(parseThemes([theme({ background: { image: 'a.png', opacity: 2 } })])).toBe(
    'themes[0] background "opacity" must be 0 to 1'
  );
});
