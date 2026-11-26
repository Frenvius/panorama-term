// What a Codex run is doing, read from the session log `codex` writes as it works
// (~/.codex/sessions/YYYY/MM/DD/rollout-<local time>-<id>.jsonl).

export type FeedKind = 'message' | 'command' | 'tool' | 'edit' | 'reasoning' | 'running' | 'done';

export interface FeedLine {
  kind: FeedKind;
  text: string;
  failed?: boolean;
}

const LINE_MAX = 400;
const NAME_RE = /^rollout-(\d{4})-(\d{2})-(\d{2})T(\d{2})-(\d{2})-(\d{2})-.+\.jsonl$/;
const CMD_RE = /\bcmd\s*:\s*"((?:[^"\\]|\\.)*)"/;
// The log starts a moment after the process; a little slack covers a clock tick.
const START_SLACK_MS = 3000;

type Json = Record<string, unknown>;

const clip = (text: string) => {
  const one = text.trim();
  return one.length > LINE_MAX ? `${one.slice(0, LINE_MAX - 1)}…` : one;
};

// A script shows as its first line.
const firstLine = (text: string) => {
  const lines = text.trim().split('\n');
  return clip(lines.length > 1 ? `${lines[0]} …` : lines[0]);
};

const texts = (content: unknown): string =>
  Array.isArray(content) ? content.map((c) => (typeof (c as Json)?.text === 'string' ? (c as Json).text : '')).join('') : '';

const commandText = (command: unknown): string => {
  if (typeof command === 'string') return command;
  if (!Array.isArray(command)) return '';
  // ["/bin/bash", "-lc", "<script>"] runs the script; show only that.
  const at = command.indexOf('-lc');
  return String(at >= 0 ? (command[at + 1] ?? '') : command.join(' '));
};

const changedPaths = (changes: unknown): string[] => {
  if (Array.isArray(changes)) return changes.map((c) => String((c as Json)?.path ?? '')).filter(Boolean);
  return changes && typeof changes === 'object' ? Object.keys(changes) : [];
};

const fromItem = (item: Json): FeedLine | null => {
  switch (item.type) {
    case 'AgentMessage': {
      const text = texts(item.content);
      return text ? { kind: 'message', text: clip(text) } : null;
    }
    case 'CommandExecution': {
      const code = item.exit_code;
      return { kind: 'command', text: firstLine(commandText(item.command)), failed: typeof code === 'number' && code !== 0 };
    }
    case 'McpToolCall':
      return { kind: 'tool', text: `${String(item.server ?? '')}.${String(item.tool ?? '')}`, failed: item.status === 'failed' };
    case 'Reasoning': {
      const text = Array.isArray(item.summary_text) ? item.summary_text.join(' ') : '';
      return text ? { kind: 'reasoning', text: clip(text) } : null;
    }
    default: {
      const paths = changedPaths(item.changes);
      return paths.length ? { kind: 'edit', text: clip(paths.join(', ')) } : null;
    }
  }
};

// A call the model made whose output has not come back yet.
const pendingText = (payload: Json): string => {
  if (typeof payload.input === 'string') {
    const cmd = CMD_RE.exec(payload.input)?.[1];
    if (cmd) {
      try {
        return firstLine(JSON.parse(`"${cmd}"`) as string);
      } catch {
        return firstLine(cmd);
      }
    }
  }
  if (typeof payload.arguments === 'string') {
    try {
      const args = JSON.parse(payload.arguments) as Json;
      const text = commandText(args.command ?? args.cmd);
      if (text) return firstLine(text);
    } catch {
      // Not JSON: fall back to the call's name.
    }
  }
  return String(payload.name ?? 'tool');
};

// Lines of a log tail, oldest first. A tail can start mid-line; lines that do not parse are skipped.
export const parseRollout = (raw: string): FeedLine[] => {
  const lines: FeedLine[] = [];
  const pending = new Map<string, string>();
  for (const row of raw.split('\n')) {
    if (!row.startsWith('{')) continue;
    let entry: Json;
    try {
      entry = JSON.parse(row) as Json;
    } catch {
      continue;
    }
    const payload = (entry.payload ?? {}) as Json;
    const type = payload.type;
    if (type === 'item_completed' && payload.item) {
      const line = fromItem(payload.item as Json);
      if (line) lines.push(line);
    } else if (type === 'task_complete') {
      const text = clip(typeof payload.last_agent_message === 'string' ? payload.last_agent_message : '');
      // The last message usually is the final answer; it shows once, as the end.
      const last = lines.at(-1);
      if (last?.kind === 'message' && last.text === text) lines.pop();
      lines.push({ kind: 'done', text: text || 'Finished' });
    } else if ((type === 'custom_tool_call' || type === 'function_call') && typeof payload.call_id === 'string') {
      pending.set(payload.call_id, pendingText(payload));
    } else if ((type === 'custom_tool_call_output' || type === 'function_call_output') && typeof payload.call_id === 'string') {
      pending.delete(payload.call_id);
    }
  }
  const running = [...pending.values()].at(-1);
  if (running && lines.at(-1)?.kind !== 'done') lines.push({ kind: 'running', text: running });
  return lines;
};

// Day folders are walked in steps short enough to hit every day.
const STEP_MS = 6 * 3600_000;
const HEAD_TIME_RE = /^\{"timestamp":"([^"]+)"/;
// Names carry the host's local time, which may be another time zone than the client's: up to this far off.
const ZONE_MS = 15 * 3600_000;

// When a rollout file was started, from its name read as local time, in ms; NaN for other names.
export const rolloutStart = (name: string): number => {
  const m = NAME_RE.exec(name);
  if (!m) return NaN;
  const [y, mo, d, h, mi, s] = m.slice(1).map(Number);
  return new Date(y, mo - 1, d, h, mi, s).getTime();
};

// When a log began, from the timestamp its first line opens with (UTC); NaN while that is not written yet.
export const headStart = (head: string): number => Date.parse(HEAD_TIME_RE.exec(head)?.[1] ?? '');

const pad = (n: number) => String(n).padStart(2, '0');

// The day folders a run may have logged to: it can cross midnight, and the host's day can differ from the client's.
export const rolloutDirs = (home: string, startedAt: number, endedAt: number): string[] => {
  const day = (ms: number) => {
    const d = new Date(ms);
    return `${home}/.codex/sessions/${d.getFullYear()}/${pad(d.getMonth() + 1)}/${pad(d.getDate())}`;
  };
  const dirs = new Set<string>();
  for (let ms = startedAt - ZONE_MS; ms < endedAt + ZONE_MS; ms += STEP_MS) dirs.add(day(ms));
  return [...dirs.add(day(endedAt + ZONE_MS))];
};

export interface RolloutIO {
  list: (dir: string) => Promise<string[]>;
  // The start of a log, from its first line.
  start: (path: string) => Promise<number>;
}

// The log of the run that went from `startedAt` to `endedAt`: the first one begun in that span that no other task took.
export const findRollout = async (
  home: string,
  startedAt: number,
  endedAt: number,
  taken: Set<string>,
  io: RolloutIO
): Promise<string | undefined> => {
  for (const dir of rolloutDirs(home, startedAt, endedAt)) {
    const names = (await io.list(dir).catch(() => [] as string[]))
      .map((name) => ({ name, at: rolloutStart(name) }))
      .filter(({ at }) => at >= startedAt - ZONE_MS && at <= endedAt + ZONE_MS)
      .sort((a, b) => a.at - b.at);
    for (const { name } of names) {
      const path = `${dir}/${name}`;
      if (taken.has(path)) continue;
      const at = await io.start(path).catch(() => NaN);
      if (at >= startedAt - START_SLACK_MS) return at <= endedAt ? path : undefined;
    }
  }
  return undefined;
};
