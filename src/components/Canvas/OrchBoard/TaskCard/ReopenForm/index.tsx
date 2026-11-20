import React from 'react';

import styles from './styles.module.scss';

interface ReopenFormProps {
  onSubmit: (note: string) => void;
  onCancel: () => void;
  submitting: boolean;
}

const ReopenForm = ({ onSubmit, onCancel, submitting }: ReopenFormProps) => {
  const [note, setNote] = React.useState('');

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit(note.trim());
  };

  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => setNote(e.target.value);

  return (
    <form className={styles.form} onSubmit={handleSubmit}>
      <textarea
        autoFocus
        rows={2}
        value={note}
        placeholder="Reviewer note (optional)"
        className={styles.textarea}
        onChange={handleChange}
      />
      <div className={styles.actions}>
        <button type="button" className={styles.cancelBtn} onClick={onCancel}>Cancel</button>
        <button type="submit" className={styles.submitBtn} disabled={submitting}>
          {submitting ? 'Reopening...' : 'Reopen'}
        </button>
      </div>
    </form>
  );
};

export default ReopenForm;
