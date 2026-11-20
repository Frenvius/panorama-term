import ClaudeLogo from '~/components/commons/ClaudeLogo';
import { KimiLogo, PiLogo, CodexLogo, OpenCodeLogo, AntigravityLogo, GenericAgentLogo } from '~/components/commons/AgentIcons';

interface AgentLogoProps {
  agent: string;
  size?: number;
}

const AgentLogo = ({ agent, size = 16 }: AgentLogoProps) => {
  if (agent === 'claude') return <ClaudeLogo size={size} />;
  if (agent === 'codex') return <CodexLogo size={size} />;
  if (agent === 'opencode') return <OpenCodeLogo size={size} />;
  if (agent === 'pi') return <PiLogo size={size} />;
  if (agent === 'kimi') return <KimiLogo size={size} />;
  if (agent === 'antigravity') return <AntigravityLogo size={size} />;
  return <GenericAgentLogo size={size} />;
};

export default AgentLogo;
