import type { OrchAgent, OrchSettings } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { X, ChevronUp, ChevronDown } from 'lucide-react';

import settingsStyles from '~/components/commons/Settings/styles.module.scss';
import styles from '../styles.module.scss';

interface WorkersEditorProps {
  settings: OrchSettings;
  agents: OrchAgent[];
  onChangeWorkers: (workers: string[]) => void;
}

const allWorkerProfiles = (agents: OrchAgent[]): string[] => {
  const result: string[] = [];
  for (const a of agents) {
    if (a.hidden || !a.available) continue;
    if (a.models.length === 0) {
      result.push(a.id);
    } else {
      for (const m of a.models) result.push(`${a.id}:${m}`);
    }
  }
  return result;
};

const WorkersEditor = ({ settings, agents, onChangeWorkers }: WorkersEditorProps) => {
  const workerOptions = allWorkerProfiles(agents);

  const moveUp = (i: number) => {
    if (i === 0) return;
    const next = [...settings.workers];
    [next[i - 1], next[i]] = [next[i], next[i - 1]];
    onChangeWorkers(next);
  };

  const moveDown = (i: number) => {
    if (i === settings.workers.length - 1) return;
    const next = [...settings.workers];
    [next[i], next[i + 1]] = [next[i + 1], next[i]];
    onChangeWorkers(next);
  };

  const remove = (i: number) => {
    onChangeWorkers(settings.workers.filter((_, idx) => idx !== i));
  };

  const addWorker = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const val = e.target.value;
    if (!val) return;
    e.target.value = '';
    onChangeWorkers([...settings.workers, val]);
  };

  return (
    <>
      <div className={settingsStyles.group}>
        <p className={settingsStyles.groupLabel}>Worker escalation</p>
        <div className={styles.workersList}>
          {settings.workers.map((w, i) => (
            <div key={i} className={styles.workerRow}>
              <span className={styles.workerChip}>{w}</span>
              <button type="button" className={styles.arrowBtn} disabled={i === 0} onClick={() => moveUp(i)} aria-label="Move up">
                <ChevronUp size={13} strokeWidth={2} />
              </button>
              <button
                type="button"
                className={styles.arrowBtn}
                disabled={i === settings.workers.length - 1}
                onClick={() => moveDown(i)}
                aria-label="Move down"
              >
                <ChevronDown size={13} strokeWidth={2} />
              </button>
              <button type="button" className={styles.removeBtn} onClick={() => remove(i)} aria-label="Remove">
                <X size={13} strokeWidth={2} />
              </button>
            </div>
          ))}
        </div>
        {workerOptions.length > 0 && (
          <select defaultValue="" onChange={addWorker} className={styles.select}>
            <option value="" disabled>Add worker...</option>
            {workerOptions.map((opt) => (
              <option key={opt} value={opt}>{opt}</option>
            ))}
          </select>
        )}
        <p className={settingsStyles.hint}>Workers run tasks in order of escalation, from lightest to heaviest.</p>
      </div>
    </>
  );
};

export default WorkersEditor;
