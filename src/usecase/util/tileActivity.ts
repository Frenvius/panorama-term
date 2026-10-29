import type { AgentEvent } from '~/domain/interfaces/pty.interface';
import { shellSummary } from '~/usecase/util/shellSummary';

export interface TileSnippet {
  text: string;
  cmd?: string;
  role?: 'user' | 'agent';
}

export interface TileActivity {
  at?: number;
  agent?: string;
  busySince?: number;
  snippet?: TileSnippet;
}

const SNIPPET_MAX = 280;

const listeners = new Set<() => void>();
let activity = new Map<string, TileActivity>();

const commit = (id: string, next: TileActivity): void => {
  activity = new Map(activity).set(id, next);
  listeners.forEach((fn) => fn());
};

const clean = (text: string): string => text.replace(/\s+/g, ' ').trim().slice(0, SNIPPET_MAX);

export const getTileActivity = (): Map<string, TileActivity> => activity;

export const subscribeTileActivity = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const reportAgent = (id: string, agent: string | null, busy: boolean): void => {
  const prev = activity.get(id) ?? {};
  const nextAgent = agent ?? undefined;
  const busySince = busy ? (prev.busySince ?? Date.now()) : undefined;
  if (prev.agent === nextAgent && prev.busySince === busySince) return;
  commit(id, { ...prev, agent: nextAgent, busySince });
};

const eventSnippet = (evt: AgentEvent): TileSnippet | null => {
  if (evt.event === 'prompt-submit' && evt.query) return { role: 'user', text: clean(evt.query) };
  if (evt.event === 'stop' && evt.response) return { role: 'agent', text: clean(evt.response) };
  if (evt.event === 'permission') {
    const text = clean([evt.toolName, evt.message].filter(Boolean).join(': '));
    return text ? { role: 'agent', text } : null;
  }
  return null;
};

export const reportAgentEvent = (id: string, evt: AgentEvent): void => {
  const snippet = eventSnippet(evt);
  if (!snippet) return;
  commit(id, { ...activity.get(id), snippet, at: Date.now() });
};

export const reportShellLines = (id: string, lines: string[]): void => {
  const prev = activity.get(id);
  if (prev?.agent) return;
  const summary = shellSummary(lines);
  if (!summary) return;
  const snippet = { cmd: clean(summary.cmd), text: clean(summary.out) };
  if (prev?.snippet?.cmd === snippet.cmd && prev.snippet.text === snippet.text) return;
  commit(id, { ...prev, snippet, at: Date.now() });
};
