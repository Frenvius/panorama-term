import type { OrchAgent, Task } from '~/domain/interfaces/orch.interface';

import React from 'react';

import styles from './styles.module.scss';
import { listAgents } from '~/adapter/orch/orch.client';

interface EditFormProps {
  task: Task;
  onSubmit: (spec: string, profile: string, review: boolean) => void;
  onCancel: () => void;
  submitting: boolean;
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

const EditForm = ({ task, onSubmit, onCancel, submitting }: EditFormProps) => {
  const [spec, setSpec] = React.useState(task.spec);
  const [profile, setProfile] = React.useState(task.profile ?? '');
  const [review, setReview] = React.useState(task.review);
  const [agents, setAgents] = React.useState<OrchAgent[]>([]);

  React.useEffect(() => {
    listAgents()
      .then(({ agents: a }) => setAgents(a))
      .catch(() => void 0);
  }, []);

  const agentOptions = React.useMemo(() => buildAgentOptions(agents), [agents]);

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit(spec.trim(), profile, review);
  };

  const handleSpecChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => setSpec(e.target.value);
  const handleProfileChange = (e: React.ChangeEvent<HTMLSelectElement>) => setProfile(e.target.value);
  const handleReviewChange = (e: React.ChangeEvent<HTMLInputElement>) => setReview(e.target.checked);

  return (
    <form className={styles.form} onSubmit={handleSubmit}>
      <textarea
        autoFocus
        rows={3}
        value={spec}
        placeholder="Description"
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
      <div className={styles.actions}>
        <button type="button" className={styles.cancelBtn} onClick={onCancel}>Cancel</button>
        <button type="submit" className={styles.submitBtn} disabled={submitting}>
          {submitting ? 'Saving...' : 'Save'}
        </button>
      </div>
    </form>
  );
};

export default EditForm;
