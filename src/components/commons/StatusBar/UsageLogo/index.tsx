import type { UsageProvider } from '~/domain/interfaces/usage.interface';

import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { CodexLogo, AntigravityLogo } from '~/components/commons/AgentIcons';

interface UsageLogoProps {
  provider: UsageProvider;
  size: number;
}

const UsageLogo = ({ provider, size }: UsageLogoProps) => {
  if (provider === 'claude') return <ClaudeLogo size={size} />;
  if (provider === 'antigravity') return <AntigravityLogo size={size} />;
  return <CodexLogo size={size} />;
};

export default UsageLogo;
