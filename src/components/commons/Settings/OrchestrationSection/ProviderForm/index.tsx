import type { OrchPreset, OrchProvider } from '~/domain/interfaces/orch.interface';
import type { SaveProviderOpts } from '~/adapter/orch/orch.client';

import React from 'react';

import { testProvider } from '~/adapter/orch/orch.client';
import Button from '~/components/commons/Settings/Button';
import Field, { Input } from '~/components/commons/Settings/Field';
import Select from '~/components/commons/Settings/Select';
import styles from '../styles.module.scss';

interface ProviderFormProps {
  presets: OrchPreset[];
  provider?: OrchProvider | null;
  onSave: (opts: SaveProviderOpts) => Promise<void>;
  onDelete?: (id: string) => Promise<void>;
  onCancel: () => void;
}

const ProviderForm = ({ presets, provider, onSave, onDelete, onCancel }: ProviderFormProps) => {
  const initial = provider ?? presets[0];
  const [preset, setPreset] = React.useState(provider?.preset ?? presets[0]?.id ?? 'custom');
  const [label, setLabel] = React.useState(initial?.label ?? '');
  const [baseUrl, setBaseUrl] = React.useState(initial?.baseUrl ?? '');
  const [mainModel, setMainModel] = React.useState(initial?.mainModel ?? '');
  const [fastModel, setFastModel] = React.useState(initial?.fastModel ?? '');
  const [apiKey, setApiKey] = React.useState('');
  const [saving, setSaving] = React.useState(false);
  const [testing, setTesting] = React.useState(false);
  const [saveError, setSaveError] = React.useState('');
  const [testResult, setTestResult] = React.useState<{ ok: boolean; message: string } | null>(null);

  const presetOptions = presets.map((p) => ({ value: p.id, label: p.label }));

  const applyPreset = (id: string) => {
    const previous = presets.find((pr) => pr.id === preset);
    setPreset(id);
    const p = presets.find((pr) => pr.id === id);
    if (!p) return;
    if (!label || label === previous?.label) setLabel(p.label);
    setBaseUrl(p.baseUrl);
    setMainModel(p.mainModel);
    setFastModel(p.fastModel);
  };

  const handleSave = () => {
    setSaving(true);
    setSaveError('');
    const opts: SaveProviderOpts = { preset, label, baseUrl, mainModel, fastModel };
    if (provider?.id) opts.id = provider.id;
    if (apiKey) opts.apiKey = apiKey;
    onSave(opts)
      .catch((e: unknown) => setSaveError(e instanceof Error ? e.message : String(e)))
      .finally(() => setSaving(false));
  };

  const handleTest = () => {
    if (!provider?.id) return;
    setTesting(true);
    setTestResult(null);
    testProvider(provider.id)
      .then((r) => setTestResult({ ok: r.reachable, message: r.message }))
      .catch((e: unknown) => setTestResult({ ok: false, message: e instanceof Error ? e.message : String(e) }))
      .finally(() => setTesting(false));
  };

  const handleDelete = () => {
    if (!provider?.id || !onDelete) return;
    void onDelete(provider.id);
  };

  return (
    <div className={styles.form}>
      <p className={styles.formTitle}>{provider ? `Edit ${provider.label}` : 'Add provider'}</p>
      <div className={styles.formFields}>
        <Field label="Preset">
          <Select options={presetOptions} value={preset} onChange={applyPreset} />
        </Field>
        <Field label="Label">
          <Input value={label} onChange={setLabel} placeholder="My provider" />
        </Field>
        <Field label="Base URL">
          <Input value={baseUrl} onChange={setBaseUrl} placeholder="https://..." />
        </Field>
        <div className={styles.formRow}>
          <Field label="Main model">
            <Input value={mainModel} onChange={setMainModel} />
          </Field>
          <Field label="Fast model">
            <Input value={fastModel} onChange={setFastModel} />
          </Field>
        </div>
        <Field label={`API key${provider?.hasKey ? ' (leave empty to keep stored key)' : ''}`}>
          <Input
            type="password"
            value={apiKey}
            onChange={setApiKey}
            placeholder={provider?.hasKey ? '(stored)' : 'sk-...'}
          />
        </Field>
      </div>
      {saveError && <p className={styles.formError}>{saveError}</p>}
      <div className={styles.formActions}>
        <Button variant="primary" disabled={saving || !label} busy={saving} busyLabel="Saving..." onClick={handleSave}>
          Save
        </Button>
        {provider?.id && (
          <Button disabled={testing} busy={testing} busyLabel="Testing..." onClick={handleTest}>
            Test
          </Button>
        )}
        {provider?.id && onDelete && (
          <Button variant="danger" onClick={handleDelete}>Delete</Button>
        )}
        {testResult && (
          <span className={`${styles.testResult} ${testResult.ok ? styles.ok : styles.fail}`}>
            {testResult.ok ? 'Reachable' : testResult.message}
          </span>
        )}
        <Button variant="ghost" onClick={onCancel}>Cancel</Button>
      </div>
    </div>
  );
};

export default ProviderForm;
