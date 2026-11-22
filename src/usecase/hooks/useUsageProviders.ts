import type { UsageProvider } from '~/domain/interfaces/usage.interface';

import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { USAGE_PROVIDERS, STATUS_BAR_USAGE_KEY } from '~/usecase/util/constants';

const listeners = new Set<() => void>();

let enabled: UsageProvider[] | null = null;

const read = (): UsageProvider[] => {
  enabled ??= getSetting<UsageProvider[]>(STATUS_BAR_USAGE_KEY, USAGE_PROVIDERS);
  return enabled;
};

const subscribe = (onChange: () => void): (() => void) => {
  listeners.add(onChange);
  return () => listeners.delete(onChange);
};

export const setUsageProviders = (next: UsageProvider[]): void => {
  enabled = next;
  void setSetting(STATUS_BAR_USAGE_KEY, next);
  listeners.forEach((l) => l());
};

export const toggleUsageProvider = (provider: UsageProvider): void => {
  const current = read();
  setUsageProviders(current.includes(provider) ? current.filter((p) => p !== provider) : [...current, provider]);
};

export const useUsageProviders = (): UsageProvider[] => React.useSyncExternalStore(subscribe, read);
