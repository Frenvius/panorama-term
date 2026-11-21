import React from 'react';
import { X } from 'lucide-react';

import ModelCombobox from '~/components/commons/ModelCombobox';
import styles from './styles.module.scss';
import { createTask } from '~/adapter/orch/orch.client';
import { useOrchSettings } from '~/usecase/hooks/useOrchSettings';

interface TaskFormProps {
  runId: string;
  onClose: () => void;
}

const TaskForm = ({ runId, onClose }: TaskFormProps) => {
  const [title, setTitle] = React.useState('');
  const [spec, setSpec] = React.useState('');
  const [profile, setProfile] = React.useState('');
  const [review, setReview] = React.useState(false);
  const [submitting, setSubmitting] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const { agents } = useOrchSettings();

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
      {profile && (
        <div className={styles.profileChip}>
          <span>{profile}</span>
          <button type="button" onClick={() => setProfile('')}>
            <X size={10} strokeWidth={2} />
          </button>
        </div>
      )}
      <ModelCombobox agents={agents} placeholder={profile ? 'Change agent...' : 'Default agent'} onPick={setProfile} />
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
