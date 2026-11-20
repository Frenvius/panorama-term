import type { Role, OrchAgent } from '~/domain/interfaces/orch.interface';

import { X, ChevronUp, ChevronDown } from 'lucide-react';

import Select from '~/components/commons/Settings/Select';
import styles from '../styles.module.scss';

interface RoleChainEditorProps {
  role: Role;
  chain: string[];
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

const RoleChainEditor = ({ role, chain, agents, onChange }: RoleChainEditorProps) => {
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

  const addEntry = (val: string) => {
    if (!val) return;
    onChange([...chain, val]);
  };

  const selectOptions = profileOptions
    .filter((p) => !chain.includes(p))
    .map((p) => ({ value: p, label: p }));

  return (
    <div data-role={role}>
      {chain.length === 0 ? (
        <p className={styles.chainEmpty}>No models in chain.</p>
      ) : (
        <div className={styles.workersList}>
          {chain.map((entry, i) => {
            const handleMoveUp = () => moveUp(i);
            const handleMoveDown = () => moveDown(i);
            const handleRemove = () => remove(i);
            return (
              <div key={i} className={styles.workerRow}>
                <span className={styles.workerChip}>{entry}</span>
                <button
                  type="button"
                  className={styles.arrowBtn}
                  disabled={i === 0}
                  onClick={handleMoveUp}
                  aria-label="Move up"
                >
                  <ChevronUp size={13} strokeWidth={2} />
                </button>
                <button
                  type="button"
                  className={styles.arrowBtn}
                  disabled={i === chain.length - 1}
                  onClick={handleMoveDown}
                  aria-label="Move down"
                >
                  <ChevronDown size={13} strokeWidth={2} />
                </button>
                <button type="button" className={styles.removeBtn} onClick={handleRemove} aria-label="Remove">
                  <X size={13} strokeWidth={2} />
                </button>
              </div>
            );
          })}
        </div>
      )}
      {selectOptions.length > 0 && (
        <div className={styles.chainAddRow}>
          <Select
            options={[{ value: '', label: 'Add model...' }, ...selectOptions]}
            value=""
            onChange={addEntry}
            placeholder="Add model..."
          />
        </div>
      )}
    </div>
  );
};

export default RoleChainEditor;
