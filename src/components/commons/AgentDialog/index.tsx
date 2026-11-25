import type { Effort } from '~/domain/interfaces/orch.interface';
import type { PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';
import { FolderOpen } from 'lucide-react';

import styles from './styles.module.scss';
import Dialog from '~/components/commons/Dialog';
import { agentActions } from '~/usecase/hooks/useAgents';
import { useOrchSettings } from '~/usecase/hooks/useOrchSettings';
import ModelCombobox from '~/components/commons/ModelCombobox';
import { createAgent, updateAgent } from '~/adapter/agents/agents.client';
import { isValidAgentName, utf8ByteLength } from '~/usecase/util/agents';

const INSTRUCTION_CAP = 4096;

const samePath = (p: string): string => p.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
const EFFORTS: Effort[] = ['low', 'medium', 'high', 'xhigh', 'max'];

interface AgentDialogProps {
  agent?: PersistentAgent;
  defaultCwd: string;
  onClose: () => void;
  onSaved: (agent: PersistentAgent) => void;
  onFolderPick: () => Promise<string | null>;
}

const AgentDialog = ({ agent, defaultCwd, onClose, onSaved, onFolderPick }: AgentDialogProps) => {
  const isEdit = agent != null;
  const fieldId = React.useId();
  const { agents: orchAgents } = useOrchSettings();

  const [name, setName] = React.useState(agent?.name ?? '');
  const [title, setTitle] = React.useState(agent?.title ?? '');
  const [description, setDescription] = React.useState(agent?.description ?? '');
  const [profile, setProfile] = React.useState(
    agent ? (agent.model ? `${agent.profile}:${agent.model}` : agent.profile) : ''
  );
  const [effort, setEffort] = React.useState<Effort | ''>(
    (agent?.effort as Effort | null | undefined) ?? ''
  );
  const [cwd, setCwd] = React.useState(agent?.cwd ?? defaultCwd);
  const [instructions, setInstructions] = React.useState(agent?.instructions ?? '');
  const [autoStart, setAutoStart] = React.useState(agent?.autoStart ?? true);
  const [submitting, setSubmitting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const instrBytes = utf8ByteLength(instructions);
  const instrOver = instrBytes > INSTRUCTION_CAP;

  const colonIdx = profile.indexOf(':');
  const profileId = colonIdx >= 0 ? profile.slice(0, colonIdx) : profile;
  const modelVal = colonIdx >= 0 ? profile.slice(colonIdx + 1) : undefined;

  const cwdChanged = isEdit && agent != null && samePath(cwd) !== samePath(agent.cwd);
  const profileChanged = isEdit && agent != null && profileId !== agent.profile;
  const modelChanged = isEdit && agent != null && (modelVal || null) !== agent.model;
  const sessionWillDrop = Boolean(agent?.nativeSession) && (cwdChanged || profileChanged || modelChanged);

  const nameValid = isValidAgentName(name);
  const canSubmit = nameValid && Boolean(profileId) && Boolean(cwd) && !instrOver && !submitting;

  const handleNameChange = (e: React.ChangeEvent<HTMLInputElement>) => setName(e.target.value);
  const handleTitleChange = (e: React.ChangeEvent<HTMLInputElement>) => setTitle(e.target.value);
  const handleDescChange = (e: React.ChangeEvent<HTMLInputElement>) => setDescription(e.target.value);
  const handleEffortChange = (e: React.ChangeEvent<HTMLSelectElement>) => setEffort(e.target.value as Effort | '');
  const handleInstrChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => setInstructions(e.target.value);
  const handleAutoStartChange = (e: React.ChangeEvent<HTMLInputElement>) => setAutoStart(e.target.checked);

  const handlePickFolder = () => {
    onFolderPick().then((p) => { if (p) setCwd(p); }).catch(() => {});
  };

  const handleSubmit = () => {
    if (!canSubmit) return;
    setSubmitting(true);
    setError(null);
    if (isEdit && agent != null) {
      updateAgent({
        id: agent.id,
        name: name !== agent.name ? name : undefined,
        title,
        description,
        profile: profileId || undefined,
        model: modelVal || null,
        effort: effort || null,
        cwd,
        instructions,
        autoStart,
      })
        .then(({ agent: saved, restartNeeded }) => {
          if (restartNeeded) agentActions.markRestartNeeded(saved.id);
          onSaved(saved);
        })
        .catch((err) => {
          setError(err instanceof Error ? err.message : String(err));
          setSubmitting(false);
        });
    } else {
      createAgent({
        name,
        title: title || undefined,
        description: description || undefined,
        profile: profileId,
        model: modelVal || undefined,
        effort: effort || undefined,
        cwd,
        instructions: instructions || undefined,
        autoStart,
      })
        .then(({ agent: saved }) => onSaved(saved))
        .catch((err) => {
          setError(err instanceof Error ? err.message : String(err));
          setSubmitting(false);
        });
    }
  };

  const footer = (
    <>
      {error && <span className={styles.error}>{error}</span>}
      <button type="button" className={styles.btn} onClick={onClose}>Cancel</button>
      <button type="submit" className={`${styles.btn} ${styles.primary}`} disabled={!canSubmit}>
        {submitting ? (isEdit ? 'Saving...' : 'Creating...') : (isEdit ? 'Save' : 'Create')}
      </button>
    </>
  );

  return (
    <Dialog title={isEdit ? 'Edit agent' : 'New agent'} onClose={onClose} onSubmit={handleSubmit} footer={footer}>
      <div className={styles.fields}>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={`${fieldId}-name`}>Name</label>
          <input
            id={`${fieldId}-name`}
            type="text"
            className={`${styles.input}${name && !nameValid ? ` ${styles.invalid}` : ''}`}
            value={name}
            placeholder="my-agent"
            autoFocus={!isEdit}
            onChange={handleNameChange}
          />
          {name && !nameValid && <span className={styles.hint}>Lowercase letters, digits and hyphens only</span>}
        </div>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={`${fieldId}-title`}>Title</label>
          <input
            id={`${fieldId}-title`}
            type="text"
            className={styles.input}
            value={title}
            placeholder="Short role description"
            maxLength={60}
            onChange={handleTitleChange}
          />
        </div>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={`${fieldId}-description`}>Description</label>
          <input
            id={`${fieldId}-description`}
            type="text"
            className={styles.input}
            value={description}
            placeholder="Optional"
            maxLength={280}
            onChange={handleDescChange}
          />
        </div>
        <div className={styles.field}>
          <label className={styles.label}>Agent / model</label>
          <ModelCombobox agents={orchAgents} value={profile || undefined} placeholder="Select agent..." onPick={setProfile} />
        </div>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={`${fieldId}-effort`}>Effort</label>
          <select id={`${fieldId}-effort`} className={styles.select} value={effort} onChange={handleEffortChange}>
            <option value="">Default</option>
            {EFFORTS.map((e) => (
              <option key={e} value={e}>{e}</option>
            ))}
          </select>
        </div>
        <div className={styles.field}>
          <label className={styles.label} htmlFor={`${fieldId}-cwd`}>Working directory</label>
          <div className={styles.cwdRow}>
            <input id={`${fieldId}-cwd`} type="text" className={styles.input} value={cwd} readOnly />
            <button type="button" className={styles.browseBtn} onClick={handlePickFolder} aria-label="Browse">
              <FolderOpen size={14} strokeWidth={1.75} />
            </button>
          </div>
          {sessionWillDrop && (
            <span className={styles.warning}>Changing this will clear the current session.</span>
          )}
        </div>
        <div className={styles.field}>
          <label className={styles.label}>
            Instructions
            <span className={instrOver ? styles.counterOver : styles.counter}>{instrBytes} / {INSTRUCTION_CAP}</span>
          </label>
          <textarea
            className={`${styles.textarea}${instrOver ? ` ${styles.invalid}` : ''}`}
            value={instructions}
            rows={5}
            placeholder="Optional agent instructions..."
            onChange={handleInstrChange}
          />
        </div>
        <label className={styles.checkLabel}>
          <input type="checkbox" checked={autoStart} onChange={handleAutoStartChange} className={styles.check} />
          Start automatically
        </label>
      </div>
    </Dialog>
  );
};

export default AgentDialog;
