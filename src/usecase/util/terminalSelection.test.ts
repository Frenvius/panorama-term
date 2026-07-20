import { expect, test } from 'bun:test';

import { urlSpanAt, orderSel, selectText, lineSelection, wordSelection } from '~/usecase/util/terminalSelection';

const lines = ['hello world   ', 'foo bar', 'last line here'];

test('orderSel normalizes reversed selection', () => {
  const r = orderSel({ a: { row: 2, col: 3 }, b: { row: 0, col: 1 } });
  expect(r.s).toEqual({ row: 0, col: 1 });
  expect(r.e).toEqual({ row: 2, col: 3 });
});

test('single-row selection slices inclusive and rtrims', () => {
  expect(selectText(lines, 14, { a: { row: 0, col: 0 }, b: { row: 0, col: 4 } })).toBe('hello');
});

test('multi-row selection joins with newline, rtrimmed', () => {
  const sel = { a: { row: 0, col: 6 }, b: { row: 2, col: 3 } };
  expect(selectText(lines, 14, sel)).toBe('world\nfoo bar\nlast');
});

test('selection past line end pads then rtrims to empty', () => {
  expect(selectText([''], 10, { a: { row: 0, col: 0 }, b: { row: 0, col: 9 } })).toBe('');
});

test('wordSelection grabs a plain word', () => {
  const line = 'foo bar baz';
  const w = wordSelection(line, 0, 5);
  expect(selectText([line], 11, w)).toBe('bar');
});

test('wordSelection keeps path chars together', () => {
  const line = 'cd /usr/local/bin here';
  const w = wordSelection(line, 0, 6);
  expect(selectText([line], 22, w)).toBe('/usr/local/bin');
});

test('wordSelection on whitespace grabs the space run', () => {
  const w = wordSelection('a   b', 0, 2);
  expect(w).toEqual({ a: { row: 0, col: 1 }, b: { row: 0, col: 3 } });
});

test('lineSelection copies whole line rtrimmed', () => {
  const line = 'echo hi        ';
  expect(selectText([line], 15, lineSelection(0, 15))).toBe('echo hi');
});

const pad = (s: string, cols: number): string => s.padEnd(cols, ' ');

test('urlSpanAt single line', () => {
  const cols = 40;
  const span = urlSpanAt([pad('open https://example.com/foo here', cols)], cols, 0, 12);
  expect(span?.url).toBe('https://example.com/foo');
  expect(span?.segments.length).toBe(1);
});

test('urlSpanAt reconstructs a url wrapped across rows', () => {
  const cols = 20;
  const full = 'https://example.com/a/very/long/path/that/wraps?q=1234567890';
  const rows: string[] = [];
  for (let i = 0; i < full.length; i += cols) rows.push(full.slice(i, i + cols));
  const lines = rows.map((r) => pad(r, cols));

  const onLast = urlSpanAt(lines, cols, rows.length - 1, 2);
  expect(onLast?.url).toBe(full);
  expect(onLast?.segments.length).toBe(rows.length);

  expect(urlSpanAt(lines, cols, 0, 0)?.url).toBe(full);
});

test('urlSpanAt strips trailing punctuation', () => {
  const cols = 40;
  const span = urlSpanAt([pad('see (https://example.com/x).', cols)], cols, 0, 6);
  expect(span?.url).toBe('https://example.com/x');
});
