import { expect, test } from 'bun:test';

import { parseFonts } from '~/usecase/util/pluginFont';

const font = (over: Record<string, unknown> = {}) => ({ family: 'Iosevka Term', files: ['fonts/regular.woff2'], ...over });

test('files can be a plain path or carry weight and style', () => {
  const fonts = parseFonts([
    font({ monospace: true, files: ['fonts/regular.woff2', { path: 'fonts/bold.ttf', weight: 700, style: 'italic' }] })
  ]);
  expect(fonts).toEqual([
    {
      family: 'Iosevka Term',
      monospace: true,
      files: [
        { path: 'fonts/regular.woff2', weight: '400', style: 'normal' },
        { path: 'fonts/bold.ttf', weight: '700', style: 'italic' }
      ]
    }
  ]);
  expect(parseFonts(undefined)).toEqual([]);
});

test('bad fonts explain what is wrong', () => {
  expect(parseFonts({})).toBe('"fonts" must be a list');
  expect(parseFonts([1])).toBe('fonts[0] must be an object');
  expect(parseFonts([font({ family: "Evil'; x" })])).toBe(
    'fonts[0] "family" must use letters, digits, spaces, dots, dashes or underscores'
  );
  expect(parseFonts([font({ monospace: 'yes' })])).toBe('fonts[0] "monospace" must be true or false');
  expect(parseFonts([font({ files: [] })])).toBe('fonts[0] "files" must list at least one file');
  expect(parseFonts([font({ files: [3] })])).toBe('fonts[0] files[0] must be a path or an object');
  for (const path of ['../x.woff2', '/abs/x.woff2', 'a/../../x.ttf', 'C:/x.ttf', 'fonts\\x.ttf', 'fonts/x.png', 'fonts//x.ttf']) {
    expect(parseFonts([font({ files: [path] })])).toBe(
      'fonts[0] files[0] must be a .woff2, .woff, .ttf or .otf file inside the plugin'
    );
  }
  expect(parseFonts([font({ files: [{ path: 'a.ttf', weight: 750 }] })])).toBe('fonts[0] files[0] "weight" must be 100 to 900');
  expect(parseFonts([font({ files: [{ path: 'a.ttf', style: 'oblique' }] })])).toBe(
    'fonts[0] files[0] "style" must be "normal" or "italic"'
  );
  expect(parseFonts([font(), font()])).toBe('fonts[1] repeats the family "Iosevka Term"');
});
