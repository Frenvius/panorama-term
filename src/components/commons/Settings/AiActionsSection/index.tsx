import type { AiTarget, OrchAgent, OrchProvider } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { orchSettingsActions, useOrchSettings } from '~/usecase/hooks/useOrchSettings';
import ModelCombobox from '~/components/commons/ModelCombobox';
import Select from '~/components/commons/Settings/Select';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import styles from './styles.module.scss';

const DEFAULT_AI_ACTIONS = {
  default: { provider: 'claude', model: 'haiku' },
  overrides: {} as Record<string, AiTarget>
};

const buildProviderOptions = (providers: OrchProvider[]) => [
  { value: 'claude', label: 'Claude Code' },
  { value: 'codex', label: 'Codex' },
  { value: 'antigravity', label: 'Antigravity' },
  ...providers.map((p) => ({ value: p.id, label: p.label }))
];

const buildOverrideOptions = (providers: OrchProvider[]) => [
  { value: '', label: 'Use default' },
  ...buildProviderOptions(providers)
];

const defaultModelFor = (provider: string): string => (provider === 'claude' ? 'haiku' : '');

interface ModelFieldProps {
  target: AiTarget;
  agents: OrchAgent[];
  onChange: (t: AiTarget) => void;
}

const ModelField = ({ target, agents, onChange }: ModelFieldProps) => {
  const providerAgents = React.useMemo(
    () => agents.filter((a) => a.id === target.provider).map((a) => ({ ...a, hidden: false })),
    [agents, target.provider]
  );

  const onPick = (value: string) => {
    const colon = value.indexOf(':');
    onChange({ ...target, model: colon < 0 ? '' : value.slice(colon + 1) });
  };

  return <ModelCombobox agents={providerAgents} value={target.model} placeholder="CLI default model" onPick={onPick} />;
};

interface TargetControlProps {
  target: AiTarget;
  agents: OrchAgent[];
  providers: OrchProvider[];
  onChange: (t: AiTarget) => void;
}

const TargetControl = ({ target, agents, providers, onChange }: TargetControlProps) => {
  const onProvider = (v: string) => onChange({ provider: v, model: defaultModelFor(v) });

  return (
    <div className={styles.targetControl}>
      <Select options={buildProviderOptions(providers)} value={target.provider} onChange={onProvider} />
      <ModelField target={target} agents={agents} onChange={onChange} />
    </div>
  );
};

interface OverrideControlProps {
  override: AiTarget | undefined;
  agents: OrchAgent[];
  providers: OrchProvider[];
  onChange: (t: AiTarget | undefined) => void;
}

const OverrideControl = ({ override, agents, providers, onChange }: OverrideControlProps) => {
  const onProvider = (v: string) => onChange(v ? { provider: v, model: defaultModelFor(v) } : undefined);

  return (
    <div className={styles.targetControl}>
      <Select options={buildOverrideOptions(providers)} value={override?.provider ?? ''} onChange={onProvider} />
      {override && <ModelField target={override} agents={agents} onChange={onChange} />}
    </div>
  );
};

const AiActionsSection = () => {
  const { settings, agents, loading } = useOrchSettings();

  if (loading && !settings) {
    return (
      <Group label="Default">
        <Row label="Loading..." />
      </Group>
    );
  }

  if (!settings) {
    return (
      <Group label="Default">
        <Row label="Unable to load settings." description="Is the brain running?" />
      </Group>
    );
  }

  const aiActions = settings.aiActions ?? DEFAULT_AI_ACTIONS;
  const providers = settings.providers;

  const saveDefault = (target: AiTarget) => {
    void orchSettingsActions.updateSettings({ aiActions: { ...aiActions, default: target } });
  };

  const saveOverride = (key: 'commitMessage' | 'tidy', target: AiTarget | undefined) => {
    const overrides = { ...aiActions.overrides };
    if (target) overrides[key] = target;
    else delete overrides[key];
    void orchSettingsActions.updateSettings({ aiActions: { ...aiActions, overrides } });
  };

  const saveCommit = (t: AiTarget | undefined) => saveOverride('commitMessage', t);
  const saveTidy = (t: AiTarget | undefined) => saveOverride('tidy', t);

  return (
    <>
      <Group label="Default">
        <Row
          settingId="aiActions.default"
          label="Default"
          description="Used when no action-specific provider is set."
          stacked
        >
          <TargetControl target={aiActions.default} agents={agents} providers={providers} onChange={saveDefault} />
        </Row>
      </Group>
      <Group label="Actions">
        <Row
          settingId="aiActions.commitMessage"
          label="Commit message"
          description="Provider for generating commit messages."
          stacked
        >
          <OverrideControl override={aiActions.overrides.commitMessage} agents={agents} providers={providers} onChange={saveCommit} />
        </Row>
        <Row
          settingId="aiActions.tidy"
          label="Tidy projects"
          description="Provider for the tidy layout action."
          stacked
        >
          <OverrideControl override={aiActions.overrides.tidy} agents={agents} providers={providers} onChange={saveTidy} />
        </Row>
      </Group>
    </>
  );
};

export default AiActionsSection;
