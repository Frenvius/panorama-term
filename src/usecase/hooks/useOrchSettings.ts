import type { OrchAgent, OrchPreset, OrchSettings } from '~/domain/interfaces/orch.interface';
import type { SaveProviderOpts, SetSettingsOpts } from '~/adapter/orch/orch.client';

import React from 'react';

import { getSettings, setSettings, saveProvider, deleteProvider, listAgents } from '~/adapter/orch/orch.client';

interface OrchSettingsState {
  agents: OrchAgent[];
  error: string | null;
  loading: boolean;
  presets: OrchPreset[];
  settings: OrchSettings | null;
  refreshingModels: boolean;
}

const DEFAULT: OrchSettingsState = {
  agents: [],
  error: null,
  loading: false,
  presets: [],
  settings: null,
  refreshingModels: false
};

let state: OrchSettingsState = DEFAULT;

type Listener = () => void;
const listeners = new Set<Listener>();

const notify = (): void => {
  for (const fn of listeners) fn();
};

const setError = (err: unknown): void => {
  state = { ...state, loading: false, error: err instanceof Error ? err.message : String(err) };
  notify();
};

const getSnapshot = (): OrchSettingsState => state;

const load = (): void => {
  state = { ...state, loading: true, error: null };
  notify();
  Promise.all([getSettings(), listAgents()])
    .then(([s, a]) => {
      state = { settings: s.settings, presets: s.presets, agents: a.agents, loading: false, error: null, refreshingModels: false };
      notify();
    })
    .catch(setError);
};

let subscriberCount = 0;

const subscribe = (fn: Listener): (() => void) => {
  listeners.add(fn);
  subscriberCount++;
  if (subscriberCount === 1) load();
  return () => {
    listeners.delete(fn);
    subscriberCount--;
  };
};

const refreshAgents = (): void => {
  listAgents()
    .then((r) => {
      state = { ...state, agents: r.agents };
      notify();
    })
    .catch(() => {});
};

export const orchSettingsActions = {
  refresh: load,

  refreshModels: (): void => {
    state = { ...state, refreshingModels: true, error: null };
    notify();
    listAgents({ refresh: true })
      .then((r) => {
        state = { ...state, agents: r.agents, refreshingModels: false };
        notify();
      })
      .catch((err) => {
        state = { ...state, refreshingModels: false, error: err instanceof Error ? err.message : String(err) };
        notify();
      });
  },

  updateSettings: (opts: SetSettingsOpts): Promise<void> =>
    setSettings(opts).then((r) => {
      state = { ...state, settings: r.settings };
      notify();
      refreshAgents();
    }).catch(setError),

  saveProvider: (opts: SaveProviderOpts): Promise<void> =>
    saveProvider(opts).then((r) => {
      if (!state.settings) return;
      const providers = state.settings.providers.some((p) => p.id === r.provider.id)
        ? state.settings.providers.map((p) => (p.id === r.provider.id ? r.provider : p))
        : [...state.settings.providers, r.provider];
      state = { ...state, settings: { ...state.settings, providers } };
      notify();
      refreshAgents();
    }).catch(setError),

  deleteProvider: (id: string): Promise<void> =>
    deleteProvider(id).then(() => {
      if (!state.settings) return;
      const providers = state.settings.providers.filter((p) => p.id !== id);
      state = { ...state, settings: { ...state.settings, providers } };
      notify();
      refreshAgents();
    }).catch(setError),

  addCustomModel: (agentId: string, model: string): Promise<void> => {
    if (!state.settings) {
      setError(new Error('orch settings not loaded'));
      return Promise.resolve();
    }
    const current = state.settings.customModels ?? {};
    const existing = current[agentId] ?? [];
    if (existing.includes(model)) return Promise.resolve();
    const next = { ...current, [agentId]: [...existing, model] };
    return setSettings({ customModels: next }).then((r) => {
      state = { ...state, settings: r.settings };
      notify();
      refreshAgents();
    }).catch(setError);
  },

  removeCustomModel: (agentId: string, model: string): Promise<void> => {
    if (!state.settings) {
      setError(new Error('orch settings not loaded'));
      return Promise.resolve();
    }
    const current = state.settings.customModels ?? {};
    const existing = current[agentId] ?? [];
    const updated = existing.filter((m) => m !== model);
    const next = updated.length > 0
      ? { ...current, [agentId]: updated }
      : Object.fromEntries(Object.entries(current).filter(([k]) => k !== agentId));
    return setSettings({ customModels: next }).then((r) => {
      state = { ...state, settings: r.settings };
      notify();
      refreshAgents();
    }).catch(setError);
  }
};

export const useOrchSettings = (): OrchSettingsState => {
  return React.useSyncExternalStore(subscribe, getSnapshot);
};
