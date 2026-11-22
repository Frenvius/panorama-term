export type UsageProvider = 'codex' | 'claude' | 'antigravity';

export interface UsageWindow {
  key: string;
  used: number;
  label: string;
  title?: string;
  resetsAt: number | null;
}

export interface ProviderUsage {
  error: string | null;
  windows: UsageWindow[];
  provider: UsageProvider;
}
