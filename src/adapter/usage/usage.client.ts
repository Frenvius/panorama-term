import type { UsageProvider } from '~/domain/interfaces/usage.interface';

import { invoke } from '~/adapter/transport';

export const fetchAgentUsage = (provider: UsageProvider, accountId?: string): Promise<Record<string, unknown>> =>
  invoke('agent_usage', accountId ? { provider, accountId } : { provider });
