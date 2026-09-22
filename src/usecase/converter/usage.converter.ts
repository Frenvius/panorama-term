import type { UsageWindow } from '~/domain/interfaces/usage.interface';

type Json = Record<string, any>;

const DAY_SECONDS = 86400;

const toMs = (value: unknown): number | null => {
  if (typeof value === 'number') return value > 1e10 ? value : value * 1000;
  if (typeof value !== 'string') return null;
  const ms = new Date(value).getTime();
  return Number.isNaN(ms) ? null : ms;
};

const clamp = (n: number) => Math.min(100, Math.max(0, n));

const claudeWindow = (key: string, label: string, raw: Json | null | undefined): UsageWindow | null =>
  typeof raw?.utilization === 'number' ? { key, label, used: clamp(raw.utilization), resetsAt: toMs(raw.resets_at) } : null;

export const claudeUsageConverter = (data: Json): UsageWindow[] => {
  const fable = (data.limits as Json[] | undefined)?.find(
    (l) => l?.kind === 'weekly_scoped' && l.scope?.model?.display_name?.toLowerCase() === 'fable'
  );
  return [
    claudeWindow('session', '5h', data.five_hour),
    claudeWindow('weekly', 'wk', data.seven_day),
    fable ? claudeWindow('fable', 'Fable', { resets_at: fable.resets_at, utilization: fable.percent }) : null
  ].filter((w): w is UsageWindow => w !== null);
};

export const codexUsageConverter = (data: Json): UsageWindow[] =>
  [data.rate_limit?.primary_window, data.rate_limit?.secondary_window]
    .filter((w: Json | null | undefined): w is Json => typeof w?.used_percent === 'number')
    .map((w) => {
      const weekly = (w.limit_window_seconds ?? 0) > DAY_SECONDS;
      return {
        label: weekly ? 'wk' : '5h',
        resetsAt: toMs(w.reset_at),
        used: clamp(w.used_percent),
        key: weekly ? 'weekly' : 'session'
      };
    })
    .sort((a, b) => (a.key === 'session' ? -1 : b.key === 'session' ? 1 : 0));
