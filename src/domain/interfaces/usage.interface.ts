export type UsageProvider = 'codex' | 'claude';

export interface UsageWindow {
  key: string;
  used: number;
  label: string;
  resetsAt: number | null;
}

export interface ProviderUsage {
  error: string | null;
  windows: UsageWindow[];
  provider: UsageProvider;
}
