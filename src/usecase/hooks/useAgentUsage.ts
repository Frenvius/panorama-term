import type { UsageWindow, ProviderUsage, UsageProvider } from '~/domain/interfaces/usage.interface';

import React from 'react';

import { USAGE_PROVIDERS } from '~/usecase/util/constants';
import { reportUsage } from '~/adapter/orch/orch.client';
import { fetchAgentUsage } from '~/adapter/usage/usage.client';
import { listAccounts } from '~/adapter/accounts/accounts.client';
import { codexUsageConverter, claudeUsageConverter, antigravityUsageConverter } from '~/usecase/converter/usage.converter';

const POLL_MS = 300000;

const CONVERTERS: Record<UsageProvider, (data: any) => UsageWindow[]> = {
  claude: claudeUsageConverter,
  codex: codexUsageConverter,
  antigravity: antigravityUsageConverter
};

const SYSTEM_ACCOUNT = 'system';

const defaultAccountId = async (provider: UsageProvider): Promise<string> => {
  const reply = await listAccounts();
  if (provider === 'antigravity') return reply.liveAgy ?? SYSTEM_ACCOUNT;
  return reply.defaults[provider] || SYSTEM_ACCOUNT;
};

const pushSnapshot = async (provider: UsageProvider, windows: UsageWindow[]): Promise<void> => {
  const accountId = await defaultAccountId(provider).catch(() => SYSTEM_ACCOUNT);
  await reportUsage({
    agent: provider,
    accountId,
    windows: windows.map(({ key, used, resetsAt }) => ({ key, used, resetsAt }))
  });
};

const load = async (provider: UsageProvider): Promise<ProviderUsage> => {
  try {
    const data = await fetchAgentUsage(provider);
    const windows = CONVERTERS[provider](data);
    void pushSnapshot(provider, windows).catch(() => {});
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
