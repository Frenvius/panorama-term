import { describe, expect, it } from 'bun:test';

import { countFrameInputChars, countInputChars, countInputImages, isAgentBusy } from '~/components/Terminal/AgentBar/parse';

import type { GridFrame } from '~/domain/interfaces/pty.interface';

const RULE = '─'.repeat(86);

const box = (...inner: string[]): string[] => [
  '  some output above                                                                   ',
  RULE,
  ...inner,
  RULE,
  '  [Opus 5 (1M context)]        12%                                                    ',
  '  ⏵⏵ auto mode on (shift+tab to cycle) · ← for agents                                 '
];

const inputFrame = (text: string, cursorCol: number, dim: boolean): GridFrame => {
  const lines = box(`❯ ${text}`);
  const cols = Math.max(...lines.map((line) => Array.from(line).length));
  const padded = lines.map((line) => line.padEnd(cols));
  const attrs = new Uint32Array(padded.length * cols * 2);
  for (let row = 0; row < padded.length; row++) {
    for (let col = 0; col < cols; col++) attrs[(row * cols + col) * 2] = 0xc7d0e0;
  }
  if (dim) {
    for (let col = 2; col < 2 + text.length; col++) attrs[(2 * cols + col) * 2] = (1 << 27) | 0x606570;
  }
  return {
    rows: padded.length,
    cols,
    cursorRow: 2,
    cursorCol,
    cursorHidden: false,
    mouseMode: 0,
    offset: 0,
    lines: padded,
    attrs
  };
};

describe('readInputText', () => {
  it('returns 0 without an input box', () => {
    expect(countInputChars(['just output'])).toBe(0);
  });

  it('returns 0 for an empty box', () => {
    expect(countInputChars(box('❯                                    '))).toBe(0);
  });

  it('ignores the placeholder', () => {
    expect(countInputChars(box('❯ Try "fix the build"                '))).toBe(0);
  });

  it('ignores arbitrary dim prompt suggestions reported by the grid', () => {
    expect(countFrameInputChars(inputFrame('manda a mensagem pro fabiano', 2, true))).toBe(0);
  });

  it('recognizes dim suggestions from sidecars that only report the blended color', () => {
    const frame = inputFrame('manda a mensagem pro fabiano', 2, true);
    for (let i = 0; i < frame.attrs.length; i += 2) frame.attrs[i] &= ~(1 << 27);
    expect(countFrameInputChars(frame)).toBe(0);
  });

  it('counts real input even when it matches a previous suggestion', () => {
    const text = 'manda a mensagem pro fabiano';
    expect(countFrameInputChars(inputFrame(text, text.length + 2, false))).toBe(text.length);
  });

  it('counts a single line', () => {
    expect(countInputChars(box('❯ hello                              '))).toBe(5);
  });

  it('counts wrapped lines, never under the real length', () => {
    const rows = box('❯ são a mesma coisa,                 ', '  só o pmo_id no projeto             ');
    expect(countInputChars(rows)).toBeGreaterThanOrEqual(43);
  });

  it('still reads the rounded-border variant', () => {
    expect(countInputChars(['╭────────╮', '│ > hello        │', '╰────────╯'])).toBe(5);
  });

  it('counts image chips', () => {
    const rows = box('❯ look [Image #12] and               ', '  [Image #13]                        ');
    expect(countInputImages(rows)).toBe(2);
    expect(countInputImages(box('❯                                    '))).toBe(0);
  });
});

describe('isAgentBusy', () => {
  it('detects the running spinner', () => {
    expect(isAgentBusy(['✢ Sublimating… (1m 27s · ↓ 3.5k tokens)'])).toBe(true);
    expect(isAgentBusy(['* Thinking... (esc to interrupt)'])).toBe(true);
  });

  it('does not fire on a finished turn', () => {
    expect(isAgentBusy(['✻ Cogitated for 5s', ...box('❯ hi')])).toBe(false);
  });
});
