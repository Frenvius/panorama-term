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

const AGY_WINDOWS: Record<string, { key: string; label: string; title: string }> = {
  '5h': { key: 'session', label: '5h', title: 'Session' },
  weekly: { key: 'weekly', label: 'wk', title: 'Weekly' }
};

const poolName = (group: string) => group.replace(/\s+models$/i, '');

export const antigravityUsageConverter = (data: Json): UsageWindow[] => {
  const tightest = new Map<string, UsageWindow>();
  for (const group of (data.groups as Json[] | undefined) ?? []) {
    for (const bucket of (group?.buckets as Json[] | undefined) ?? []) {
      const win = AGY_WINDOWS[bucket?.window];
      if (!win || bucket.disabled || typeof bucket.remaining_fraction !== 'number') continue;
      const used = clamp(Math.round((1 - bucket.remaining_fraction) * 100));
      const prev = tightest.get(win.key);
      if (prev && prev.used >= used) continue;
      const resetsAt = toMs(bucket.reset_time);
      tightest.set(win.key, {
        used,
        key: win.key,
        label: win.label,
        resetsAt: resetsAt && resetsAt > 0 ? resetsAt : null,
        title: `${win.title} · ${poolName(String(group.name ?? ''))}`
      });
    }
  }
  return ['session', 'weekly'].map((k) => tightest.get(k)).filter((w): w is UsageWindow => w !== undefined);
};
