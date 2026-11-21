import type { Task } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { X } from 'lucide-react';

import ModelCombobox from '~/components/commons/ModelCombobox';
import styles from './styles.module.scss';
import { useOrchSettings } from '~/usecase/hooks/useOrchSettings';

interface EditFormProps {
  task: Task;
  onSubmit: (spec: string, profile: string, review: boolean) => void;
  onCancel: () => void;
  submitting: boolean;
}

const EditForm = ({ task, onSubmit, onCancel, submitting }: EditFormProps) => {
  const [spec, setSpec] = React.useState(task.spec);
  const [profile, setProfile] = React.useState(task.profile ?? '');
  const [review, setReview] = React.useState(task.review);
  const { agents } = useOrchSettings();

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit(spec.trim(), profile, review);
  };

  const handleSpecChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => setSpec(e.target.value);
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
