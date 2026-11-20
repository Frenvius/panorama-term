import type { OrchPreset, OrchProvider } from '~/domain/interfaces/orch.interface';
import type { SaveProviderOpts } from '~/adapter/orch/orch.client';

import React from 'react';

import { testProvider } from '~/adapter/orch/orch.client';
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
  const [testResult, setTestResult] = React.useState<{ ok: boolean; message: string } | null>(null);

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
    const opts: SaveProviderOpts = { preset, label, baseUrl, mainModel, fastModel };
    if (provider?.id) opts.id = provider.id;
    if (apiKey) opts.apiKey = apiKey;
    onSave(opts).finally(() => setSaving(false));
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
    onDelete(provider.id);
  };

  return (
    <div className={styles.form}>
      <p className={styles.formTitle}>{provider ? `Edit ${provider.label}` : 'Add provider'}</p>
      <div className={styles.field}>
        <label className={styles.fieldLabel}>Preset</label>
        <select value={preset} onChange={(e) => applyPreset(e.target.value)} className={styles.select}>
          {presets.map((p) => (
            <option key={p.id} value={p.id}>{p.label}</option>
          ))}
        </select>
      </div>
      <div className={styles.field}>
        <label className={styles.fieldLabel}>Label</label>
        <input value={label} onChange={(e) => setLabel(e.target.value)} className={styles.input} placeholder="My provider" />
      </div>
      <div className={styles.field}>
        <label className={styles.fieldLabel}>Base URL</label>
        <input value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} className={styles.input} placeholder="https://..." />
      </div>
      <div className={styles.formRow}>
        <div className={styles.field} style={{ flex: 1 }}>
          <label className={styles.fieldLabel}>Main model</label>
          <input value={mainModel} onChange={(e) => setMainModel(e.target.value)} className={styles.input} />
        </div>
        <div className={styles.field} style={{ flex: 1 }}>
          <label className={styles.fieldLabel}>Fast model</label>
          <input value={fastModel} onChange={(e) => setFastModel(e.target.value)} className={styles.input} />
        </div>
      </div>
      <div className={styles.field}>
        <label className={styles.fieldLabel}>API key{provider?.hasKey ? ' (leave empty to keep stored key)' : ''}</label>
        <input
          type="password"
          value={apiKey}
          onChange={(e) => setApiKey(e.target.value)}
          className={styles.input}
          placeholder={provider?.hasKey ? '(stored)' : 'sk-...'}
        />
      </div>
      <div className={styles.formActions}>
        <button type="button" className={styles.saveBtn} disabled={saving || !label} onClick={handleSave}>
          {saving ? 'Saving...' : 'Save'}
        </button>
        {provider?.id && (
          <button type="button" className={styles.testBtn} disabled={testing} onClick={handleTest}>
            {testing ? 'Testing...' : 'Test'}
          </button>
        )}
        {provider?.id && onDelete && (
          <button type="button" className={styles.deleteBtn} onClick={handleDelete}>Delete</button>
        )}
        {testResult && (
          <span className={`${styles.testResult} ${testResult.ok ? styles.ok : styles.fail}`}>
            {testResult.ok ? 'Reachable' : testResult.message}
          </span>
        )}
        <button type="button" className={styles.cancelBtn} onClick={onCancel}>Cancel</button>
      </div>
    </div>
  );
};

export default ProviderForm;
