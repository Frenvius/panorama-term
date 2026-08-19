import type { FooterRead, ParsedStatus, SuggestTrigger } from './types';

const PERMISSION_MODE_LABELS: Record<string, string> = {
  auto: 'auto',
  plan: 'plan',
  normal: 'normal',
  default: 'normal',
  acceptedits: 'accept edits',
  bypasspermissions: 'bypass permissions'
};

export const modeKey = (mode: string): string => {
  const m = mode.toLowerCase();
  if (m.includes('plan')) return 'plan';
  if (m.includes('accept')) return 'accept';
  if (m.includes('bypass')) return 'bypass';
  if (m.includes('auto')) return 'auto';
  return 'default';
};

export const prettyMode = (raw: string | undefined): string | undefined => {
  if (!raw) return undefined;
  const key = raw.toLowerCase().replace(/[\s_-]/g, '');
  return PERMISSION_MODE_LABELS[key] ?? raw.toLowerCase().replace(/\s+/g, ' ');
};

export const prettyModel = (raw: string | undefined): { model?: string; contextInfo?: string } => {
  if (!raw) return {};
  const beta = raw.match(/\[([^\]]+)\]/);
  const contextInfo = beta?.[1] && /1m/i.test(beta[1]) ? '1M context' : undefined;
  const label = raw
    .replace(/\[[^\]]*\]/, '')
    .replace(/-\d{6,8}$/, '')
    .split('-')
    .filter(Boolean)
    .map((s) => s.charAt(0).toUpperCase() + s.slice(1))
    .join('-');
  const out: { model?: string; contextInfo?: string } = {};
  if (label) out.model = label;
  if (contextInfo) out.contextInfo = contextInfo;
  return out;
};

export type AgentType = 'claude' | 'antigravity' | 'codex' | 'opencode' | 'pi' | 'generic';

const AGENT_UI = /[╭╮╰╯]|\besc to interrupt\b|\?\s*for shortcuts|auto-?accept edits|auto mode on|⏵⏵|bypass permissions|plan mode on|for agents\b|to cycle\)|press ctrl-?c again|\d+(?:\.\d+)?%\/\d+(?:\.\d+)?[kM]|\[[^\]\n]*\b(opus|sonnet|haiku|fable|gpt|gemini)\b[^\]\n]*\]/i;

export const hasAgentUi = (text: string): boolean => AGENT_UI.test(text);

const PI_STATS = /(?:\d+\.\d|\?)%\/(\d+(?:\.\d)?[kMB])(?: \(auto\))?(?:\s{2,}|\s*$)/;

const AGENT_SIGNATURES: [AgentType, RegExp][] = [
  ['claude', /welcome to claude code|claude code v\d|claude\.ai\/|anthropic\.com\/(?:s\/)?claude-code|\/help for help/i],
  ['codex', /openai codex|codex cli|\bcodex\b\s+v\d|>_\s*codex/i],
  ['antigravity', /welcome to antigravity|antigravity cli|\bantigravity\b\s+v\d|>_\s*antigravity/i],
  ['opencode', /welcome to opencode|opencode cli|\bopencode\b\s+v\d/i],
  ['pi', PI_STATS]
];

const AGENT_MODEL_HINTS: [AgentType, RegExp][] = [
  ['claude', /\[[^\]\n]*\b(opus|sonnet|haiku|fable)\b[^\]\n]*\]|\b(opus|sonnet|haiku)\s+[\d.]+/i],
  ['codex', /\bgpt-[\d.]|\bo\d-(?:mini|preview)\b/i],
  ['antigravity', /\bgemini[\s-][\d.]/i]
];

const DECLARED_AGENTS: AgentType[] = ['claude', 'antigravity', 'codex', 'opencode', 'pi', 'generic'];

export const declaredAgent = (state: ClaudeState | null): AgentType | null => {
  if (!state) return null;
  const declared = DECLARED_AGENTS.find((type) => type === state.agent);
  return declared ?? 'claude';
};

export const detectAgentIdentity = (text: string): AgentType | null =>
  (AGENT_SIGNATURES.find(([, re]) => re.test(text)) ?? AGENT_MODEL_HINTS.find(([, re]) => re.test(text)))?.[0] ?? null;

const PLACEHOLDER_MODEL = /^<.+>$/;

const parsePiStatus = (lines: string[]): ParsedStatus | null => {
  const stats = lines.find((line) => PI_STATS.test(line));
  const context = stats?.match(PI_STATS);
  if (!stats || !context) return null;
  const result: ParsedStatus = {};
  if (context[1]) result.contextInfo = context[1];
  const percent = stats.match(/(\d+\.\d)%\//);
  if (percent?.[1]) result.progress = Math.round(parseFloat(percent[1]));
  const right = stats.slice(stats.indexOf(context[0]) + context[0].length).trim();
  const [name, thinking] = right.replace(/^\([^)]*\)\s*/, '').split('•');
  const model = name?.trim();
  if (model && !PLACEHOLDER_MODEL.test(model)) result.model = model;
  const level = thinking?.trim();
  if (level && level !== 'thinking off') result.mode = level;
  return result;
};

export const parseStatusLines = (lines: string[]): ParsedStatus => {
  if (lines.length === 0) return {};
  const pi = parsePiStatus(lines);
  if (pi) return pi;
  const combined = lines.join(' ').replace(/\s+/g, ' ');
  const firstLine = lines[0] ?? '';
  const result: ParsedStatus = {};

  const modelMatch = firstLine.match(/^\s*\[([^\]]+?)(?:\s*\(([^)]+)\))?\]/);
  if (modelMatch?.[1]) {
    result.model = modelMatch[1].trim();
    if (modelMatch[2]) result.contextInfo = modelMatch[2].trim();
  }

  const progressMatch = firstLine.match(/\][^%]*?(\d{1,3})\s*%/);
  if (progressMatch?.[1]) result.progress = parseInt(progressMatch[1], 10);

  const modeMatch = combined.match(/([\w\s-]+?)\s+on\s+\(\S+\s+to\s+cycle\)/i);
  if (modeMatch?.[1]) {
    result.mode = modeMatch[1].trim().toLowerCase().replace(/\s+/g, ' ').replace(/\s*mode$/, '');
  }

  if (/\bfocus\b/i.test(combined)) result.focused = true;

  return result;
};

const scrapeModel = (rows: string[]): { model: string; contextInfo?: string } | undefined => {
  for (let i = rows.length - 1; i >= 0; i--) {
    const m = rows[i]?.match(/Set model to (.+?)\s+and saved/i);
    if (m?.[1]) {
      const raw = m[1].trim();
      const ctx = raw.match(/\(([^)]+)\)/)?.[1];
      const name = raw.replace(/\s*\([^)]*\)\s*/, '').trim();
      return ctx ? { model: name, contextInfo: ctx } : { model: name };
    }
  }
  for (let i = 0; i < rows.length - 1; i++) {
    if (!/Claude Code v\d/i.test(rows[i] ?? '')) continue;
    const banner = rows[i + 1] ?? '';
    const bm = banner.match(/\b(Opus|Sonnet|Haiku|Fable)\s+[\d.]+/i);
    if (bm) {
      const ctx = banner.match(/\(([^)]*context[^)]*)\)/i)?.[1];
      return ctx ? { model: bm[0].trim(), contextInfo: ctx.trim() } : { model: bm[0].trim() };
    }
    break;
  }
  return undefined;
};

const isBoxBottom = (s: string): boolean => s.includes('╰') && s.includes('╯');
const isBoxTop = (s: string): boolean => s.includes('╭') && s.includes('╮');

const PLACEHOLDER = /^(try\b|write a prompt|ask\b|\/ for commands|\? for shortcuts)/i;

const isInputBorder = (s: string): boolean => {
  const trimmed = s.trim();
  return /^[─━-]{10,}$/.test(trimmed) || isBoxTop(trimmed) || isBoxBottom(trimmed);
};

const inputBounds = (rows: string[]): { top: number; bottom: number } | null => {
  let bottom = -1;
  for (let i = rows.length - 1; i >= 0; i--) {
    if (isInputBorder(rows[i] ?? '')) {
      bottom = i;
      break;
    }
  }
  if (bottom < 1) return null;
  for (let i = bottom - 1; i >= 0; i--) {
    if (isInputBorder(rows[i] ?? '')) return { top: i, bottom };
  }
  return null;
};

export const readInputText = (rows: string[]): string => {
  const bounds = inputBounds(rows);
  if (!bounds) return '';
  const { top, bottom } = bounds;
  const lines: string[] = [];
  for (let i = top + 1; i < bottom; i++) {
    const inner = (rows[i] ?? '')
      .replace(/^\s*[│┃]/, '')
      .replace(/[│┃]\s*$/, '')
      .replace(/^\s*[❯>]\s?/, '')
      .trimEnd();
    lines.push(inner);
  }
  while (lines.length > 0 && !lines[lines.length - 1]?.trim()) lines.pop();
  if (lines.length === 0) return '';
  if (lines.length === 1 && PLACEHOLDER.test(lines[0]?.trim() ?? '')) return '';
  return lines.join('\n');
};

export const countInputChars = (rows: string[]): number => readInputText(rows).length;

const DIM_FLAG = 1 << 27;
const LEGACY_DIM_FG = 0x606570;

export const isInputSuggestion = (frame: GridFrame): boolean => {
  const bounds = inputBounds(frame.lines);
  if (!bounds) return false;
  const promptRow = frame.lines.findIndex((row, i) => i > bounds.top && i < bounds.bottom && /^\s*[│┃]?\s*[❯>]/.test(row));
  if (promptRow < 0 || frame.cursorRow !== promptRow) return false;
  const cells = Array.from(frame.lines[promptRow] ?? '');
  const promptCol = cells.findIndex((cell) => cell === '❯' || cell === '>');
  if (promptCol < 0) return false;
  let contentCol = promptCol + 1;
  while (/\s/.test(cells[contentCol] ?? '')) contentCol++;
  if (frame.cursorCol > contentCol) return false;
  let visible = 0;
  for (let row = promptRow; row < bounds.bottom; row++) {
    const rowCells = Array.from(frame.lines[row] ?? '');
    const start = row === promptRow ? contentCol : 0;
    for (let col = start; col < frame.cols; col++) {
      const cell = rowCells[col] ?? ' ';
      if (!cell.trim() || cell === '│' || cell === '┃') continue;
      visible++;
      const attr = frame.attrs[(row * frame.cols + col) * 2] ?? 0;
      if ((attr & DIM_FLAG) === 0 && (attr & 0x00ff_ffff) !== LEGACY_DIM_FG) return false;
    }
  }
  return visible > 0;
};

export const countFrameInputChars = (frame: GridFrame | null): number =>
  !frame || isInputSuggestion(frame) ? 0 : countInputChars(frame.lines);

export const isAgentBusy = (rows: string[]): boolean =>
  rows.some((row) => /esc to interrupt|ctrl\+b to run in background|…\s*\(/i.test(row));

export const countInputImages = (rows: string[]): number =>
  readInputText(rows).match(/\[Image #\d+\]/g)?.length ?? 0;

export const readFooter = (rows: string[]): FooterRead => {
  const model = scrapeModel(rows);

  const status: string[] = [];
  let cursor = rows.length - 1;
  while (cursor >= 0 && status.length < 2) {
    const row = rows[cursor] ?? '';
    if (isInputBorder(row)) break;
    if (row.trim()) status.unshift(row);
    cursor--;
  }

  const above = rows.slice(0, cursor + 1);
  const hasInputBox = above.some(isInputBorder);

  const statusText = status.join(' ');
  const hasStatusMarker = /\[[^\]]+\]/.test(statusText);
  const hasFocusMarker = /\bfocus\b/i.test(statusText);
  const hasModeBanner = rows.some((line) =>
    /mode on|to cycle\)|esc to interrupt|for agents|accept edits|bypass permissions|plan mode|⏵⏵|\?\s*for shortcuts|Claude Code v\d/i.test(
      line
    )
  );
  const menuMode = rows.some((line) =>
    /\b(resume session|select a|select an|switch to|choose)\b|\(\s*\d+\s+of\s+\d+\s*\)|to show all projects|only show current branch/i.test(
      line
    )
  );
  const exitBanner = rows.some((line) => /press ctrl-?c again/i.test(line));

  const questionMode =
    !hasStatusMarker && !hasFocusMarker && !hasModeBanner && (menuMode || (!exitBanner && !hasInputBox));

  const uiPresent =
    hasInputBox || hasStatusMarker || hasFocusMarker || hasModeBanner || menuMode || exitBanner;

  return model ? { status, uiPresent, questionMode, model } : { status, uiPresent, questionMode };
};

export const detectSuggestTrigger = (text: string, caret: number): SuggestTrigger => {
  const before = text.slice(0, caret);
  const modelMatch = before.match(/^\/model\s+(\S*)$/);
  if (modelMatch) return { kind: 'model', query: modelMatch[1] ?? '' };
  const effortMatch = before.match(/^\/effort\s+(\S*)$/);
  if (effortMatch) return { kind: 'effort', query: effortMatch[1] ?? '' };
  if (/^\/\S*$/.test(before)) return { kind: 'slash', query: before.slice(1) };
  return null;
};
import type { ClaudeState, GridFrame } from '~/domain/interfaces/pty.interface';
