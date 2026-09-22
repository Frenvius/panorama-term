import type { ProviderUsage, UsageProvider } from '~/domain/interfaces/usage.interface';

import React from 'react';

import { USAGE_PROVIDERS } from '~/usecase/util/constants';
import { fetchAgentUsage } from '~/adapter/usage/usage.client';
import { codexUsageConverter, claudeUsageConverter } from '~/usecase/converter/usage.converter';

const POLL_MS = 300000;

const load = async (provider: UsageProvider): Promise<ProviderUsage> => {
  try {
    const data = await fetchAgentUsage(provider);
    const windows = provider === 'claude' ? claudeUsageConverter(data) : codexUsageConverter(data);
    return { windows, provider, error: null };
  } catch (e) {
    return { provider, windows: [], error: String(e) };
  }
};

export const useAgentUsage = (providers: UsageProvider[]) => {
  const [usage, setUsage] = React.useState<ProviderUsage[]>([]);
  const [loading, setLoading] = React.useState(false);
  const key = providers.join(',');

  const refresh = React.useCallback(() => {
    const active = USAGE_PROVIDERS.filter((p) => key.split(',').includes(p));
    setLoading(true);
    void Promise.all(active.map(load))
      .then(setUsage)
      .finally(() => setLoading(false));
  }, [key]);

  React.useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  return { usage, loading, refresh };
};
