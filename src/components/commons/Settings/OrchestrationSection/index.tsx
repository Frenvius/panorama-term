import type { OrchProvider, Role, OrchestrateHint, RoleChains } from '~/domain/interfaces/orch.interface';
import type { SaveProviderOpts } from '~/adapter/orch/orch.client';

import React from 'react';
import { Plus, RefreshCw } from 'lucide-react';

import { ROLES, ROLE_META } from '~/usecase/util/orch';
import { useOrchModelStatus } from '~/usecase/hooks/useOrchModelStatus';
import { orchSettingsActions, useOrchSettings } from '~/usecase/hooks/useOrchSettings';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import AgentLogo from '~/components/commons/AgentLogo';
import Switch from '~/components/commons/Settings/Switch';
import Button from '~/components/commons/Settings/Button';
import Segmented from '~/components/commons/Settings/Segmented';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';
import ProviderForm from './ProviderForm';
import ModelStatus from './ModelStatus';
import ModelHistory from './ModelHistory';
import RoleChainEditor from './RoleChainEditor';
import styles from './styles.module.scss';

const HINT_OPTIONS = [
  { value: 'off', label: 'Off' },
  { value: 'suggest', label: 'Suggest' },
  { value: 'auto', label: 'Auto' }
];

const HINT_DESCRIPTIONS: Record<OrchestrateHint, string> = {
  off: 'No orchestration hint is injected.',
  auto: 'Agents orchestrate large work without asking.',
  suggest: 'Agents propose orchestrating large work and wait for your approval.'
};

const OrchestrationSection = () => {
  const { settings, presets, agents, loading, refreshingModels } = useOrchSettings();
  const [editingProvider, setEditingProvider] = React.useState<OrchProvider | null | 'new'>(null);
  const [deleteTarget, setDeleteTarget] = React.useState<string | null>(null);
  const { candidates, metrics, clear: clearCandidateState } = useOrchModelStatus();

  const saveRoleChain = (role: Role, chain: string[]) => {
    const roles = { [role]: chain } as Partial<RoleChains>;
    void orchSettingsActions.updateSettings({ roles });
  };

  const setOrchestrateHint = (hint: string) => {
    void orchSettingsActions.updateSettings({ orchestrateHint: hint as OrchestrateHint });
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

  const confirmDelete = (id: string) => setDeleteTarget(id);
  const cancelDelete = () => setDeleteTarget(null);

  const doDeleteProvider = (): Promise<void> => {
    if (!deleteTarget) return Promise.resolve();
    return orchSettingsActions.deleteProvider(deleteTarget).then(() => {
      setDeleteTarget(null);
      setEditingProvider(null);
    });
  };

  const handleDeleteProvider = (id: string): Promise<void> => {
    confirmDelete(id);
    return Promise.resolve();
  };

  const openAdd = () => setEditingProvider('new');
  const closeForm = () => setEditingProvider(null);

  if (loading && !settings) {
    return (
      <Group label="Runs">
        <Row label="Loading..." />
      </Group>
    );
  }

  if (!settings) {
    return (
      <Group label="Runs">
        <Row label="Unable to load settings." description="Is the brain running?" />
      </Group>
    );
  }

  const hint = settings.orchestrateHint ?? 'suggest';
  const builtinAgents = agents.filter((a) => a.kind === 'builtin');

  return (
    <>
      {deleteTarget && (
        <ConfirmDialog
          title="Delete provider"
          message="Delete this provider? The API key will be removed."
          confirmLabel="Delete"
          danger
          onConfirm={doDeleteProvider}
          onCancel={cancelDelete}
        />
      )}
      <Group label="Runs">
        <Row
          settingId="orchestration.workerTiles"
          label="Open worker tiles automatically"
          description="Off: workers run in the background; open one from the run board."
        >
          <Switch checked={settings.showWorkerTiles} onChange={toggleShowWorkerTiles} label="Open worker tiles automatically" />
        </Row>
        <Row
          settingId="orchestration.hint"
          label="Orchestrate hint"
          description={HINT_DESCRIPTIONS[hint]}
        >
          <Segmented options={HINT_OPTIONS} value={hint} onChange={setOrchestrateHint} label="Orchestrate hint" />
        </Row>
      </Group>
      <Group
        label="Roles"
        action={
          <Button
            size="sm"
            icon={RefreshCw}
            disabled={refreshingModels}
            busy={refreshingModels}
            busyLabel="Refreshing..."
            onClick={orchSettingsActions.refreshModels}
          >
            Refresh models
          </Button>
        }
      >
        {ROLES.map((role) => {
          const handleChange = (chain: string[]) => saveRoleChain(role, chain);
          const chain = settings.roles?.[role] ?? [];
          return (
            <div key={role}>
              <Row
                settingId="orchestration.roles"
                label={ROLE_META[role].label}
                description={ROLE_META[role].description}
                stacked
              >
                <RoleChainEditor role={role} chain={chain} agents={agents} onChange={handleChange} />
              </Row>
            </div>
          );
        })}
        <div className={styles.chainCaption}>
          Order is preference. Operational failures fall back down the list without spending attempts; quality failures move past the failed model.
        </div>
      </Group>
      <ModelStatus candidates={candidates} onClear={clearCandidateState} />
      <ModelHistory rows={metrics} />
      <Group
        label="Providers"
        action={
          <Button size="sm" icon={Plus} onClick={openAdd}>Add provider</Button>
        }
      >
        {settings.providers.length === 0 && editingProvider !== 'new' && (
          <Row label="No custom providers." description="Add one to use external Claude-compatible APIs." />
        )}
        {settings.providers.map((p) => {
          const handleRowClick = () => setEditingProvider(editingProvider !== 'new' && editingProvider?.id === p.id ? null : p);
          return (
            <React.Fragment key={p.id}>
              <div
                className={`${styles.providerRow} ${editingProvider && editingProvider !== 'new' && editingProvider.id === p.id ? styles.active : ''}`}
                onClick={handleRowClick}
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
          );
        })}
        {editingProvider === 'new' && (
          <ProviderForm presets={presets} onSave={handleSaveProvider} onCancel={closeForm} />
        )}
      </Group>
      {builtinAgents.length > 0 && (
        <Group label="Available agents">
          {builtinAgents.map((a) => {
            const handleToggle = () => toggleAgent(a.id);
            return (
              <Row
                key={a.id}
                label={a.label}
                icon={<AgentLogo agent={a.id} />}
                description={a.available ? undefined : 'Not found in PATH.'}
              >
                <Switch checked={!settings.hiddenAgents.includes(a.id)} onChange={handleToggle} label={a.label} />
              </Row>
            );
          })}
        </Group>
      )}
    </>
  );
};

export default OrchestrationSection;
