import type { Routine, RoutineRun, RoutineSkip, RoutineOutcome } from '~/domain/interfaces/agents.interface';

export const HISTORY_ROWS = 10;
export const NAME_MAX = 60;
export const TEXT_MAX_BYTES = 4096;

export type TriggerKind = 'daily' | 'weekdays' | 'weekly' | 'every' | 'cron' | 'hook' | 'manual';

export interface RoutineForm {
  name: string;
  text: string;
  kind: TriggerKind;
  time: string;
  days: string[];
  every: string;
  cron: string;
  missed: 'catch-up' | 'skip';
  overlap: 'skip' | 'queue';
  session: 'keep' | 'fresh' | 'rollover';
  noNote: boolean;
  reportTo: string;
  maxPerDay: number;
}

export const emptyForm = (): RoutineForm => ({
  name: '',
  text: '',
  kind: 'weekdays',
  time: '09:00',
  days: ['mon'],
  every: '1h',
  cron: '',
  missed: 'catch-up',
  overlap: 'skip',
  session: 'keep',
  noNote: false,
  reportTo: 'user',
  maxPerDay: 24
});

export interface OutcomeChip {
  kind: 'reported' | 'quiet' | 'failed' | 'skipped' | 'none';
  label: string;
}

export const outcomeChip = (outcome: RoutineOutcome | null | undefined): OutcomeChip => {
  switch (outcome) {
    case 'reported':
      return { kind: 'reported', label: 'reported' };
    case 'no_update':
      return { kind: 'quiet', label: 'no news' };
    case 'failed':
      return { kind: 'failed', label: 'failed' };
    case 'timeout':
      return { kind: 'failed', label: 'timed out' };
    case 'skipped':
      return { kind: 'skipped', label: 'skipped' };
    default:
      return { kind: 'none', label: 'not run yet' };
  }
};

export interface HistoryRow {
  key: string;
  at: number;
  chip: OutcomeChip;
  text: string;
  cost: string | null;
}

export const formatCost = (usd: number | null | undefined): string | null => (usd == null ? null : `$${usd.toFixed(usd < 0.1 ? 3 : 2)}`);

export const historyRows = (runs: RoutineRun[], skips: RoutineSkip[], limit = HISTORY_ROWS): HistoryRow[] => {
  const fromRuns: HistoryRow[] = runs.map((r) => ({
    key: `r${r.n}`,
    at: r.endedAt ?? r.deliveredAt ?? r.firedAt,
    chip: r.outcome ? outcomeChip(r.outcome) : { kind: 'none', label: r.deliveredAt ? 'running' : 'queued' },
    text: (r.summary ?? '').split('\n')[0],
    cost: formatCost(r.costUsd)
  }));
  const fromSkips: HistoryRow[] = skips.map((s, i) => ({
    key: `s${s.at}-${i}`,
    at: s.at,
    chip: outcomeChip('skipped'),
    text: s.count > 1 ? `${s.reason} (${s.count})` : s.reason,
    cost: null
  }));
  return [...fromRuns, ...fromSkips].sort((a, b) => b.at - a.at).slice(0, limit);
};

const TIME_RE = /^([01]?\d|2[0-3]):[0-5]\d$/;
const EVERY_RE = /^\d+\s*(m|min|h|d)$/;

const everyMinutes = (raw: string): number | null => {
  const m = raw.trim().match(/^(\d+)\s*(m|min|h|d)$/);
  if (!m) return null;
  const n = Number(m[1]);
  return m[2] === 'd' ? n * 1440 : m[2] === 'h' ? n * 60 : n;
};

export const validateForm = (form: RoutineForm): string | null => {
  const name = form.name.trim();
  if (!name || name.length > NAME_MAX) return `Name must be 1 to ${NAME_MAX} characters`;
  if (/[\u0000-\u001f\u007f]/.test(name) || name.toLowerCase().includes('[panorama')) return 'Name must be plain single-line text';
  if (!form.text.trim()) return 'Write the instruction';
  if (new TextEncoder().encode(form.text).length > TEXT_MAX_BYTES) return 'Instruction is longer than 4 KB';
  if (['daily', 'weekdays', 'weekly'].includes(form.kind) && !TIME_RE.test(form.time)) return 'Time must be HH:MM';
  if (form.kind === 'weekly' && form.days.length === 0) return 'Pick at least one day';
  if (form.kind === 'every') {
    const mins = EVERY_RE.test(form.every.trim()) ? everyMinutes(form.every) : null;
    if (mins == null || mins < 5 || mins > 7 * 1440) return 'Every must be between 5m and 7d, like 30m or 2h';
  }
  if (form.kind === 'cron' && form.cron.trim().split(/\s+/).length !== 5) return 'Cron needs 5 fields: minute hour day month weekday';
  if (!Number.isInteger(form.maxPerDay) || form.maxPerDay < 1 || form.maxPerDay > 288) return 'Max per day is 1 to 288';
  return null;
};

export const formToBody = (form: RoutineForm): Record<string, unknown> => {
  const body: Record<string, unknown> = {
    name: form.name.trim(),
    text: form.text,
    missed: form.missed,
    overlap: form.overlap,
    session: form.session,
    noNote: form.session === 'rollover' && form.noNote,
    reportTo: form.reportTo,
    maxPerDay: form.maxPerDay
  };
  if (form.kind === 'daily') Object.assign(body, { at: form.time, days: 'daily' });
  if (form.kind === 'weekdays') Object.assign(body, { at: form.time, days: 'weekdays' });
  if (form.kind === 'weekly') Object.assign(body, { at: form.time, days: form.days.join(',') });
  if (form.kind === 'every') body.every = form.every.trim();
  if (form.kind === 'cron') body.cron = form.cron.trim();
  if (form.kind === 'hook') body.hook = true;
  if (form.kind === 'manual') body.manual = true;
  return body;
};

export const formFromRoutine = (r: Routine, agentNames: Record<string, string>): RoutineForm => {
  const base: RoutineForm = {
    ...emptyForm(),
    name: r.name,
    text: r.instruction,
    missed: r.missed === 'skip' ? 'skip' : 'catch-up',
    overlap: r.overlap,
    session: r.session,
    noNote: r.session === 'rollover' && r.rolloverNote === false,
    reportTo: r.reportTo.kind === 'agent' ? `agent:${agentNames[r.reportTo.id] ?? r.reportTo.id}` : r.reportTo.kind,
    maxPerDay: r.maxPerDay
  };
  if (r.trigger.kind === 'every') return { ...base, kind: 'every', every: `${Math.round(r.trigger.ms / 60_000)}m` };
  if (r.trigger.kind === 'cron') return { ...base, kind: 'cron', cron: r.trigger.expr };
  return { ...base, kind: r.trigger.kind };
};

export const hookUrl = (base: string, id: string): string => `${base}/agents/routine-hook/${id}`;
