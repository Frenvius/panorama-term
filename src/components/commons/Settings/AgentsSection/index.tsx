import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { USAGE_PROVIDERS, IDLE_COMPACTION_KEY } from '~/usecase/util/constants';
import { PROVIDER_NAMES } from '~/components/commons/StatusBar/constants';
import { useUsageProviders, toggleUsageProvider } from '~/usecase/hooks/useUsageProviders';
import { setProviderHidden, getSeenProviders, getHiddenProviders } from '~/usecase/util/agentProviders';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Switch from '~/components/commons/Settings/Switch';

const AgentsSection = () => {
  const [idleCompact, setIdleCompact] = React.useState(() => getSetting(IDLE_COMPACTION_KEY, true));
  const [providers] = React.useState(getSeenProviders);
  const [hiddenProviders, setHiddenProviders] = React.useState(getHiddenProviders);
  const usageProviders = useUsageProviders();

  const toggleIdleCompact = () => {
    const next = !idleCompact;
    setIdleCompact(next);
    void setSetting(IDLE_COMPACTION_KEY, next);
  };

  const toggleProvider = (provider: string) => {
    const hidden = !hiddenProviders.includes(provider);
    setProviderHidden(provider, hidden);
    setHiddenProviders(getHiddenProviders());
  };

  return (
    <>
      <Group label="Model picker">
        {providers.length === 0 ? (
          <Row settingId="agents.modelPicker" label="No providers seen yet" description="Open a terminal running an agent that reports its models." />
        ) : (
          providers.map((provider) => {
            const handleToggle = () => toggleProvider(provider);
            return (
              <Row key={provider} label={provider} description="List this provider's models in the model picker.">
                <Switch
                  checked={!hiddenProviders.includes(provider)}
                  onChange={handleToggle}
                  label={provider}
                />
              </Row>
            );
          })
        )}
      </Group>
      <Group label="Status bar usage">
        {USAGE_PROVIDERS.map((provider, i) => {
          const handleToggle = () => toggleUsageProvider(provider);
          return (
            <Row
              key={provider}
              settingId={i === 0 ? 'agents.usage' : undefined}
              label={PROVIDER_NAMES[provider]}
              description="Show this provider's usage limits in the status bar."
            >
              <Switch checked={usageProviders.includes(provider)} onChange={handleToggle} label={PROVIDER_NAMES[provider]} />
            </Row>
          );
        })}
      </Group>
      <Group label="Claude Code">
        <Row
          settingId="agents.idleCompaction"
          label="Idle compaction"
          description="Compact conversations over 200K tokens after about an hour idle, before the prompt cache expires."
        >
          <Switch checked={idleCompact} onChange={toggleIdleCompact} label="Idle compaction" />
        </Row>
      </Group>
    </>
  );
};

export default AgentsSection;
