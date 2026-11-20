import type { OrchProvider, Role, OrchestrateHint, RoleChains } from '~/domain/interfaces/orch.interface';
import type { SaveProviderOpts } from '~/adapter/orch/orch.client';

import React from 'react';
import { Plus } from 'lucide-react';

import { ROLES, ROLE_META } from '~/usecase/util/orch';
import { orchSettingsActions, useOrchSettings } from '~/usecase/hooks/useOrchSettings';
import ToggleOption from '~/components/commons/Settings/ToggleOption';
import settingsStyles from '~/components/commons/Settings/styles.module.scss';
import ProviderForm from './ProviderForm';
import RoleChainEditor from './RoleChainEditor';
import styles from './styles.module.scss';

const HINT_LABELS: Record<OrchestrateHint, string> = {
  off: 'Off',
  auto: 'Auto',
  suggest: 'Suggest'
};

const HINT_DESCRIPTIONS: Record<OrchestrateHint, string> = {
  off: 'No orchestration hint is injected.',
  auto: 'Agents orchestrate large work without asking.',
  suggest: 'Agents propose orchestrating large work and wait for your approval.'
};

const HINT_ORDER: OrchestrateHint[] = ['off', 'suggest', 'auto'];

const OrchestrationSection = () => {
  const { settings, presets, agents, loading } = useOrchSettings();
  const [editingProvider, setEditingProvider] = React.useState<OrchProvider | null | 'new'>(null);

  const saveRoleChain = (role: Role, chain: string[]) => {
    const roles = { [role]: chain } as Partial<RoleChains>;
    void orchSettingsActions.updateSettings({ roles });
  };

  const setOrchestrateHint = (hint: OrchestrateHint) => {
    void orchSettingsActions.updateSettings({ orchestrateHint: hint });
  };

  const toggleShowWorkerTiles = () => {
    if (!settings) return;
    void orchSettingsActions.updateSettings({ showWorkerTiles: !settings.showWorkerTiles });
  };

  const toggleAgent = (id: string) => {
    if (!settings) return;
    const hidden = settings.hiddenAgents.includes(id)
      ? settings.hiddenAgents.filter((a) => a !== id)
      : [...settings.hiddenAgents, id];
    void orchSettingsActions.updateSettings({ hiddenAgents: hidden });
  };

  const handleSaveProvider = (opts: SaveProviderOpts): Promise<void> =>
    orchSettingsActions.saveProvider(opts).then(() => setEditingProvider(null));

  const handleDeleteProvider = (id: string): Promise<void> =>
    orchSettingsActions.deleteProvider(id).then(() => setEditingProvider(null));

  const openAdd = () => setEditingProvider('new');
  const closeForm = () => setEditingProvider(null);

  if (loading && !settings) {
    return (
      <div className={settingsStyles.pane}>
        <div className={settingsStyles.paneHead}>
          <h2 className={settingsStyles.title}>Orchestration</h2>
        </div>
        <p className={settingsStyles.hint}>Loading...</p>
      </div>
    );
  }

  if (!settings) {
    return (
      <div className={settingsStyles.pane}>
        <div className={settingsStyles.paneHead}>
          <h2 className={settingsStyles.title}>Orchestration</h2>
        </div>
        <p className={settingsStyles.hint}>Unable to load settings. Is the brain running?</p>
      </div>
    );
  }

  const hint = settings.orchestrateHint ?? 'suggest';
  const builtinAgents = agents.filter((a) => a.kind === 'builtin');

  return (
    <div className={settingsStyles.pane}>
      <div className={settingsStyles.paneHead}>
        <h2 className={settingsStyles.title}>Orchestration</h2>
        <p className={settingsStyles.subtitle}>Agents and models used for multi-agent runs.</p>
      </div>
      <div className={settingsStyles.group}>
        <div className={settingsStyles.options}>
          <ToggleOption
            label="Open worker tiles automatically"
            checked={settings.showWorkerTiles}
            description="Off: workers run in the background; open one from the run board."
            onToggle={toggleShowWorkerTiles}
          />
        </div>
      </div>
      <div className={settingsStyles.group}>
        <p className={settingsStyles.groupLabel}>Orchestrate hint</p>
        <div className={styles.hintSegmented}>
          {HINT_ORDER.map((h) => {
            const handleHintClick = () => setOrchestrateHint(h);
            return (
              <button
                key={h}
                type="button"
                className={`${styles.hintSegBtn}${hint === h ? ` ${styles.hintSegActive}` : ''}`}
                onClick={handleHintClick}
              >
                {HINT_LABELS[h]}
              </button>
            );
          })}
        </div>
        <p className={settingsStyles.hint}>{HINT_DESCRIPTIONS[hint]}</p>
      </div>
      <div className={settingsStyles.group}>
        <p className={settingsStyles.groupLabel}>Roles</p>
        {ROLES.map((role) => {
          const handleChange = (chain: string[]) => saveRoleChain(role, chain);
          const chain = settings.roles?.[role] ?? [];
          return (
            <RoleChainEditor
              key={role}
              role={role}
              label={ROLE_META[role].label}
              description={ROLE_META[role].description}
              chain={chain}
              agents={agents}
              onChange={handleChange}
            />
          );
        })}
      </div>
      <div className={settingsStyles.group}>
        <div className={styles.providerHeader}>
          <p className={settingsStyles.groupLabel} style={{ margin: 0 }}>Providers</p>
          <button type="button" className={styles.addBtn} onClick={openAdd}>
            <Plus size={12} strokeWidth={2} />
            Add provider
          </button>
        </div>
        {settings.providers.length === 0 && editingProvider !== 'new' && (
          <p className={settingsStyles.hint}>No custom providers. Add one to use external Claude-compatible APIs.</p>
        )}
        {settings.providers.map((p) => (
          <React.Fragment key={p.id}>
            <div
              className={`${styles.providerRow} ${editingProvider && editingProvider !== 'new' && editingProvider.id === p.id ? styles.active : ''}`}
              onClick={() => setEditingProvider(editingProvider !== 'new' && editingProvider?.id === p.id ? null : p)}
            >
              <span className={`${styles.keyDot} ${p.hasKey ? styles.hasKey : ''}`} />
              <span className={styles.providerLabel}>{p.label}</span>
              <span className={styles.providerUrl}>{p.baseUrl}</span>
            </div>
            {editingProvider !== 'new' && editingProvider?.id === p.id && (
              <ProviderForm
                presets={presets}
                provider={p}
                onSave={handleSaveProvider}
                onDelete={handleDeleteProvider}
                onCancel={closeForm}
              />
            )}
          </React.Fragment>
        ))}
        {editingProvider === 'new' && (
          <ProviderForm presets={presets} onSave={handleSaveProvider} onCancel={closeForm} />
        )}
      </div>
      {builtinAgents.length > 0 && (
        <div className={settingsStyles.group}>
          <p className={settingsStyles.groupLabel}>Agents</p>
          <div className={settingsStyles.options}>
            {builtinAgents.map((a) => (
              <div key={a.id} className={styles.agentRow}>
                <ToggleOption
                  label={a.label}
                  checked={!settings.hiddenAgents.includes(a.id)}
                  description={a.available ? '' : 'Executable not found in PATH.'}
                  onToggle={() => toggleAgent(a.id)}
                />
                <span className={`${styles.badge} ${a.available ? styles.available : ''}`}>
                  {a.available ? 'found' : 'not found'}
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
};

export default OrchestrationSection;
