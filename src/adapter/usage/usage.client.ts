import type { UsageProvider } from '~/domain/interfaces/usage.interface';

import { invoke } from '@tauri-apps/api/core';

export const fetchAgentUsage = (provider: UsageProvider): Promise<Record<string, unknown>> => invoke('agent_usage', { provider });
