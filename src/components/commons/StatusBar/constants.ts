export const CLOSE_DELAY = 120;

export const PROVIDER_NAMES = { codex: 'Codex', claude: 'Claude' };

export const WINDOW_TITLES: Record<string, string> = { weekly: 'Weekly', session: 'Session', fable: 'Fable weekly' };

export const formatLeft = (ms: number) => {
  const minutes = Math.max(0, Math.round(ms / 60000));
  const days = Math.floor(minutes / 1440);
  const hours = Math.floor((minutes % 1440) / 60);
  if (days > 0) return `${days}d${hours}h`;
  return hours > 0 ? `${hours}h${minutes % 60}m` : `${minutes}m`;
};

export const formatReset = (resetsAt: number) => {
  const date = new Date(resetsAt);
  const sameDay = date.toDateString() === new Date().toDateString();
  const when = sameDay
    ? date.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' })
    : date.toLocaleString(undefined, { hour: '2-digit', weekday: 'short', minute: '2-digit' });
  return `Resets in ${formatLeft(resetsAt - Date.now())} · ${when}`;
};
