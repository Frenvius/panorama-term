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

export type AgentType = 'claude' | 'antigravity' | 'codex' | 'opencode' | 'generic';

export const detectAgent = (text: string): AgentType | null => {
  const lower = text.toLowerCase();
  if (lower.includes('antigravity') || lower.includes('agy')) return 'antigravity';
  if (lower.includes('codex') || lower.includes('github copilot') || lower.includes('copilot-cli')) return 'codex';
  if (lower.includes('opencode')) return 'opencode';
  if (lower.includes('claude code') || lower.includes('claude-code') || lower.includes('claudecode') || /\bclaude\b/i.test(text)) return 'claude';
  
  if (/[╭╮╰╯]/.test(text)) return 'generic';
  if (
    /\besc to interrupt\b|\?\s*for shortcuts|auto-?accept edits|auto mode on|⏵⏵|bypass permissions|plan mode on|for agents\b|to cycle\)/i.test(
      text
    )
  ) {
    return 'generic';
  }
  if (/\[[^\]\n]*\b(opus|sonnet|haiku|fable)\b[^\]\n]*\]/i.test(text)) return 'generic';
  if (/press ctrl-?c again/i.test(text)) return 'generic';
  return null;
};

export const detectExitBanner = (lines: string[]): boolean =>
  lines.some((line) => /press ctrl-?c again/i.test(line));

export const parseStatusLines = (lines: string[]): ParsedStatus => {
  if (lines.length === 0) return {};
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

export const readFooter = (rows: string[]): FooterRead => {
  const model = scrapeModel(rows);

  const status: string[] = [];
  let cursor = rows.length - 1;
  while (cursor >= 0 && status.length < 2) {
    const row = rows[cursor] ?? '';
    if (isBoxBottom(row) || isBoxTop(row)) break;
    if (row.trim()) status.unshift(row);
    cursor--;
  }

  const above = rows.slice(0, cursor + 1);
  const hasInputBox = above.some((r) => isBoxTop(r)) || above.some((r) => isBoxBottom(r));

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
    menuMode ||
    (!exitBanner && !hasInputBox && !hasStatusMarker && !hasFocusMarker && !hasModeBanner);

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
