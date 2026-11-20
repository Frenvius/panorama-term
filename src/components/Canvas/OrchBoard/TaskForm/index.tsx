import type { OrchAgent } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { X } from 'lucide-react';

import styles from './styles.module.scss';
import { listAgents, createTask } from '~/adapter/orch/orch.client';

interface TaskFormProps {
  runId: string;
  onClose: () => void;
}

const buildAgentOptions = (agents: OrchAgent[]): { value: string; label: string }[] => {
  const opts: { value: string; label: string }[] = [{ value: '', label: 'Default' }];
  for (const agent of agents) {
    if (!agent.available || agent.hidden) continue;
    opts.push({ value: agent.id, label: agent.label });
    for (const model of agent.models) {
      opts.push({ value: `${agent.id}:${model}`, label: `${agent.label} / ${model}` });
    }
  }
  return opts;
};

const TaskForm = ({ runId, onClose }: TaskFormProps) => {
  const [title, setTitle] = React.useState('');
  const [spec, setSpec] = React.useState('');
  const [profile, setProfile] = React.useState('');
  const [review, setReview] = React.useState(false);
  const [agents, setAgents] = React.useState<OrchAgent[]>([]);
  const [submitting, setSubmitting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    listAgents()
      .then(({ agents: a }) => setAgents(a))
      .catch(() => void 0);
  }, []);

  const agentOptions = React.useMemo(() => buildAgentOptions(agents), [agents]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) return;
    setSubmitting(true);
    setError(null);
    createTask({ runId, title: title.trim(), spec: spec.trim(), profile: profile || undefined, review, by: 'user' })
      .then(onClose)
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err));
        setSubmitting(false);
      });
  };

  const handleTitleChange = (e: React.ChangeEvent<HTMLInputElement>) => setTitle(e.target.value);
  const handleSpecChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => setSpec(e.target.value);
  const handleProfileChange = (e: React.ChangeEvent<HTMLSelectElement>) => setProfile(e.target.value);
  const handleReviewChange = (e: React.ChangeEvent<HTMLInputElement>) => setReview(e.target.checked);

  return (
    <form className={styles.form} onSubmit={handleSubmit}>
      <div className={styles.formHead}>
        <span className={styles.formTitle}>New task</span>
        <button type="button" className={styles.closeBtn} onClick={onClose}>
          <X size={11} strokeWidth={2} />
        </button>
      </div>
      <input
        autoFocus
        type="text"
        value={title}
        placeholder="Title"
        className={styles.input}
        onChange={handleTitleChange}
      />
      <textarea
        value={spec}
        rows={3}
        placeholder="Description (optional)"
        className={styles.textarea}
        onChange={handleSpecChange}
      />
      <select value={profile} className={styles.select} onChange={handleProfileChange}>
        {agentOptions.map((opt) => (
          <option key={opt.value} value={opt.value}>{opt.label}</option>
        ))}
      </select>
      <label className={styles.checkLabel}>
        <input type="checkbox" checked={review} onChange={handleReviewChange} className={styles.check} />
        Stop in review
      </label>
      {error && <div className={styles.formError}>{error}</div>}
      <div className={styles.formActions}>
        <button type="button" className={styles.cancelBtn} onClick={onClose}>Cancel</button>
        <button type="submit" className={styles.submitBtn} disabled={submitting || !title.trim()}>
          {submitting ? 'Adding...' : 'Add task'}
        </button>
      </div>
    </form>
  );
};

export default TaskForm;
