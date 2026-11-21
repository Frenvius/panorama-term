import type { Role, OrchAgent } from '~/domain/interfaces/orch.interface';

import { X, ChevronUp, ChevronDown } from 'lucide-react';

import ModelCombobox from '~/components/commons/ModelCombobox';
import styles from '../styles.module.scss';

interface RoleChainEditorProps {
  role: Role;
  chain: string[];
  agents: OrchAgent[];
  onChange: (chain: string[]) => void;
}

const RoleChainEditor = ({ role, chain, agents, onChange }: RoleChainEditorProps) => {
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
      <div className={styles.chainAddRow}>
        <ModelCombobox agents={agents} exclude={chain} placeholder="Add model..." onPick={addEntry} />
      </div>
    </div>
  );
};

export default RoleChainEditor;
