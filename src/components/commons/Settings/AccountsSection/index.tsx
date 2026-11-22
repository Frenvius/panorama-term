import type { AgentAccountKind } from '~/domain/interfaces/account.interface';

import { useAgentAccounts } from '~/usecase/hooks/useAgentAccounts';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import AccountGroup from './AccountGroup';

const AGENTS: { agent: AgentAccountKind; label: string }[] = [
  { agent: 'antigravity', label: 'Antigravity' },
  { agent: 'claude', label: 'Claude Code' },
  { agent: 'codex', label: 'Codex' }
];

const AccountsSection = () => {
  const { loaded, error } = useAgentAccounts();

  if (!loaded) {
    return (
      <Group label="Accounts">
        <Row label={error ?? 'Loading...'} description={error ? 'Is the brain running?' : undefined} />
      </Group>
    );
  }

  return (
    <>
      {error && (
        <Group label="Accounts">
          <Row label="Last action failed" description={error} />
        </Group>
      )}
      {AGENTS.map(({ agent, label }) => (
        <AccountGroup key={agent} agent={agent} label={label} />
      ))}
    </>
  );
};

export default AccountsSection;
