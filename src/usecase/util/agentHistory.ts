const RESUME: Record<string, (id: string) => string> = {
  pi: (id) => `pi --session ${id}`,
  kimi: (id) => `kimi --session ${id}`,
  codex: (id) => `codex resume ${id}`,
  claude: (id) => `claude --resume ${id}`,
  opencode: (id) => `opencode --session ${id}`
};

export const hasHistory = (agent: string | null): agent is string => agent !== null && agent in RESUME;

export const resumeCommand = (agent: string, id: string): string | null => RESUME[agent]?.(id) ?? null;

export const timeAgo = (at: string | number | null): string => {
  if (at === null) return '';
  const then = new Date(at).getTime();
  if (Number.isNaN(then)) return '';
  const mins = Math.max(0, Math.round((Date.now() - then) / 60000));
  if (mins < 1) return 'just now';
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.round(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.round(hours / 24)}d ago`;
};
