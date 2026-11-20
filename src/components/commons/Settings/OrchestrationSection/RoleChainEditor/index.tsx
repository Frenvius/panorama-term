import type { Role, OrchAgent } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { X, ChevronUp, ChevronDown } from 'lucide-react';

import settingsStyles from '~/components/commons/Settings/styles.module.scss';
import styles from '../styles.module.scss';

interface RoleChainEditorProps {
  role: Role;
  label: string;
  chain: string[];
  description: string;
  agents: OrchAgent[];
  onChange: (chain: string[]) => void;
}

const allProfiles = (agents: OrchAgent[]): string[] => {
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

const RoleChainEditor = ({ role, label, description, chain, agents, onChange }: RoleChainEditorProps) => {
  const profileOptions = allProfiles(agents);

  const moveUp = (i: number) => {
    if (i === 0) return;
    const next = [...chain];
    [next[i - 1], next[i]] = [next[i], next[i - 1]];
    onChange(next);
  };

  const moveDown = (i: number) => {
    if (i === chain.length - 1) return;
    const next = [...chain];
    [next[i], next[i + 1]] = [next[i + 1], next[i]];
    onChange(next);
  };

  const remove = (i: number) => {
    onChange(chain.filter((_, idx) => idx !== i));
  };

  const addEntry = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const val = e.target.value;
    if (!val) return;
    e.target.value = '';
    onChange([...chain, val]);
  };

  return (
    <div className={settingsStyles.group} data-role={role}>
      <p className={settingsStyles.groupLabel}>{label}</p>
      <p className={settingsStyles.hint}>{description}</p>
      <div className={styles.workersList}>
        {chain.map((entry, i) => (
          <div key={i} className={styles.workerRow}>
            <span className={styles.workerChip}>{entry}</span>
            <button
              type="button"
              className={styles.arrowBtn}
              disabled={i === 0}
              onClick={() => moveUp(i)}
              aria-label="Move up"
            >
              <ChevronUp size={13} strokeWidth={2} />
            </button>
            <button
              type="button"
              className={styles.arrowBtn}
              disabled={i === chain.length - 1}
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
      {profileOptions.length > 0 && (
        <select defaultValue="" onChange={addEntry} className={styles.select}>
          <option value="" disabled>Add entry...</option>
          {profileOptions.map((opt) => (
            <option key={opt} value={opt}>{opt}</option>
          ))}
        </select>
      )}
    </div>
  );
};

export default RoleChainEditor;
