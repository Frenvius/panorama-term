import { getSetting, setSetting } from '~/adapter/settings/settings.client';

import type { AgentModel } from '~/domain/interfaces/pty.interface';

export const AGENT_PROVIDERS_KEY = 'agentProviders';
export const AGENT_PROVIDERS_EVENT = 'panorama-agent-providers';

interface ProviderPrefs {
  seen: string[];
  hidden: string[];
}

const read = (): ProviderPrefs => ({
  seen: [],
  hidden: [],
  ...getSetting<Partial<ProviderPrefs>>(AGENT_PROVIDERS_KEY, {})
});

export const getSeenProviders = (): string[] => read().seen;

export const getHiddenProviders = (): string[] => read().hidden;

export const rememberProviders = (providers: string[]): void => {
  const prefs = read();
  const merged = [...new Set([...prefs.seen, ...providers])].sort();
  if (merged.length === prefs.seen.length) return;
  void setSetting(AGENT_PROVIDERS_KEY, { ...prefs, seen: merged });
};

export const setProviderHidden = (provider: string, hidden: boolean): void => {
  const prefs = read();
  const next = hidden ? [...new Set([...prefs.hidden, provider])] : prefs.hidden.filter((id) => id !== provider);
  void setSetting(AGENT_PROVIDERS_KEY, { ...prefs, hidden: next });
  window.dispatchEvent(new Event(AGENT_PROVIDERS_EVENT));
};

export const visibleModels = (models: AgentModel[], hidden: string[]): AgentModel[] =>
  models.filter((model) => !hidden.includes(model.provider));
