import type { AiTarget, OrchProvider } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { orchSettingsActions, useOrchSettings } from '~/usecase/hooks/useOrchSettings';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Select from '~/components/commons/Settings/Select';
import styles from './styles.module.scss';

const CLAUDE_MODELS = [
  { value: 'haiku', label: 'Haiku' },
  { value: 'sonnet', label: 'Sonnet' },
  { value: 'opus', label: 'Opus' }
];

const DEFAULT_AI_ACTIONS = {
  default: { provider: 'claude', model: 'haiku' },
  overrides: {} as Record<string, AiTarget>
};

const buildProviderOptions = (providers: OrchProvider[]) => [
  { value: 'claude', label: 'Claude Code' },
  { value: 'codex', label: 'Codex' },
  ...providers.map((p) => ({ value: p.id, label: p.label }))
];

const buildOverrideOptions = (providers: OrchProvider[]) => [
  { value: '', label: 'Use default' },
  { value: 'claude', label: 'Claude Code' },
  { value: 'codex', label: 'Codex' },
  ...providers.map((p) => ({ value: p.id, label: p.label }))
];

interface TargetControlProps {
  target: AiTarget;
  providers: OrchProvider[];
  onChange: (t: AiTarget) => void;
}

const TargetControl = ({ target, providers, onChange }: TargetControlProps) => {
  const isClaude = target.provider === 'claude';
  const opts = buildProviderOptions(providers);

  const onProvider = (v: string) => {
    onChange({ provider: v, model: v === 'claude' ? 'haiku' : target.model });
  };

  const onModel = (v: string) => onChange({ ...target, model: v });

  const onModelInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    onChange({ ...target, model: e.target.value });
  };

  return (
    <div className={styles.targetControl}>
      <Select options={opts} value={target.provider} onChange={onProvider} />
      {isClaude ? (
        <Select options={CLAUDE_MODELS} value={target.model} onChange={onModel} />
      ) : (
        <input
          className={styles.modelInput}
          value={target.model}
          onChange={onModelInput}
          placeholder="Model name"
          spellCheck={false}
        />
      )}
    </div>
  );
};

interface OverrideControlProps {
  override: AiTarget | undefined;
  providers: OrchProvider[];
  onChange: (t: AiTarget | undefined) => void;
}

const OverrideControl = ({ override, providers, onChange }: OverrideControlProps) => {
  const opts = buildOverrideOptions(providers);
  const provider = override?.provider ?? '';

  const onProvider = (v: string) => {
    if (!v) { onChange(undefined); return; }
    onChange({ provider: v, model: v === 'claude' ? 'haiku' : (override?.model ?? '') });
  };

  const onModel = (v: string) => {
    if (!override) return;
    onChange({ ...override, model: v });
  };

  const onModelInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!override) return;
    onChange({ ...override, model: e.target.value });
  };

  return (
    <div className={styles.targetControl}>
      <Select options={opts} value={provider} onChange={onProvider} />
      {override && override.provider === 'claude' && (
        <Select options={CLAUDE_MODELS} value={override.model} onChange={onModel} />
      )}
      {override && override.provider !== 'claude' && (
        <input
          className={styles.modelInput}
          value={override.model}
          onChange={onModelInput}
          placeholder="Model name"
          spellCheck={false}
        />
      )}
    </div>
  );
};

const AiActionsSection = () => {
  const { settings, loading } = useOrchSettings();

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
          <TargetControl target={aiActions.default} providers={providers} onChange={saveDefault} />
        </Row>
      </Group>
      <Group label="Actions">
        <Row
          settingId="aiActions.commitMessage"
          label="Commit message"
          description="Provider for generating commit messages."
          stacked
        >
          <OverrideControl override={aiActions.overrides.commitMessage} providers={providers} onChange={saveCommit} />
        </Row>
        <Row
          settingId="aiActions.tidy"
          label="Tidy projects"
          description="Provider for the tidy layout action."
          stacked
        >
          <OverrideControl override={aiActions.overrides.tidy} providers={providers} onChange={saveTidy} />
        </Row>
      </Group>
    </>
  );
};

export default AiActionsSection;
