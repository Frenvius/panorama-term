import { describe, expect, it } from 'bun:test';

import { aliasLabel, readFooter, parseStatusLines, countFrameInputChars, countInputChars, countInputImages, hasAgentUi, isAgentBusy, declaredAgent, detectAgentIdentity } from '~/components/Terminal/AgentBar/parse';

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

describe('readFooter questionMode', () => {
  it('ignores menu words left in restored transcript', () => {
    const rows = box('❯                                    ');
    rows.splice(0, 0, '  you can choose any of them, switch to plan (1 of 3)');
    expect(readFooter(rows).questionMode).toBe(false);
  });

  it('still fires on a real picker screen', () => {
    expect(readFooter(['  Select a session to resume', '  1. foo', '  2. bar']).questionMode).toBe(true);
  });
});


describe('pi footer', () => {
  const piScreen = [
    '  some output above',
    RULE,
    '   ',
    RULE,
    '~/workspace/projects/panorama-term (main)',
    '$0.000 (sub) 12.4%/272k (auto)                          (openai-codex) gpt-5.6-sol • medium'
  ];

  it('identifies pi from the stats line', () => {
    expect(detectAgentIdentity(piScreen.join('\n'))).toBe('pi');
  });

  it('keeps pi detected while the compaction card replaces its footer', () => {
    expect(hasAgentUi('[compaction]\nCompacted from 210.251 tokens (ctrl+o to expand)')).toBe(true);
  });

  it('reads model, context and thinking level', () => {
    const status = parseStatusLines(readFooter(piScreen).status);
    expect(status.model).toBe('gpt-5.6-sol');
    expect(status.contextInfo).toBe('272k');
    expect(status.progress).toBe(12);
    expect(status.mode).toBe('medium');
  });

  it('keeps the bar visible instead of treating the screen as a menu', () => {
    expect(readFooter(piScreen).questionMode).toBe(false);
  });
});

describe('kimi footer', () => {
  const kimiScreen = [
    '  Welcome to Kimi Code!  Send /help for help information.',
    RULE,
    '  >                                  ',
    RULE,
    '  auto plan  kimi-k2 thinking: high  panorama-term  main',
    '                                                     context: 12% (30.7k/256k)'
  ];

  it('identifies kimi instead of claude', () => {
    expect(detectAgentIdentity(kimiScreen.join('\n'))).toBe('kimi');
  });

  it('reads context usage from the second footer line', () => {
    const status = parseStatusLines(readFooter(kimiScreen).status);
    expect(status.progress).toBe(12);
    expect(status.contextInfo).toBe('256k');
  });

  it('keeps the bar visible', () => {
    expect(readFooter(kimiScreen).questionMode).toBe(false);
  });
});

describe('aliasLabel', () => {
  it('drops the provider prefix the alias already repeats', () => {
    expect(aliasLabel('claude_agent_sdk/claude-opus-5', 'claude_agent_sdk')).toBe('Claude Opus 5');
    expect(aliasLabel('openai/gpt-5', undefined)).toBe('Openai/gpt 5');
  });

  it('reads version digits and the 1M context suffix', () => {
    expect(aliasLabel('claude-opus-4-8-1m')).toBe('Claude Opus 4.8 · 1M');
    expect(aliasLabel('fable-5-1-1m')).toBe('Fable 5.1 · 1M');
    expect(aliasLabel('opus-1m')).toBe('Opus · 1M');
    expect(aliasLabel('claude-sonnet-5')).toBe('Claude Sonnet 5');
    expect(aliasLabel('gpt-5.6-sol')).toBe('GPT 5.6 Sol');
    expect(aliasLabel('haiku')).toBe('Haiku');
    expect(aliasLabel('kimi-k2')).toBe('Kimi K2');
  });

  it('keeps aliases that differ only by prefix distinguishable', () => {
    expect(aliasLabel('opus-4-8-1m')).not.toBe(aliasLabel('claude-opus-4-8-1m'));
  });
});

describe('declaredAgent', () => {
  it('trusts the agent the sidecar reports', () => {
    expect(declaredAgent({ agent: 'pi', model: 'gpt-5.6-sol' })).toBe('pi');
  });

  it('falls back to claude for the statusline, and to nothing without state', () => {
    expect(declaredAgent({ model: 'opus' })).toBe('claude');
    expect(declaredAgent({ agent: 'not-an-agent' })).toBe('claude');
    expect(declaredAgent(null)).toBe(null);
  });
});

describe('pi placeholder model', () => {
  it('ignores the bridged placeholder id and keeps the rest of the footer', () => {
    const status = parseStatusLines([
      '~/workspace/projects/panorama-term (main)',
      '$0.000 (sub) 4.0%/1.0M (auto)                          (claude-bridge) <synthetic> • medium'
    ]);
    expect(status.model).toBeUndefined();
    expect(status.contextInfo).toBe('1.0M');
    expect(status.mode).toBe('medium');
  });
});
