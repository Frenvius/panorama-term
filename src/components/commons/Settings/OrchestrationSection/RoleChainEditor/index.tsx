import type { Effort, Role, OrchAgent } from '~/domain/interfaces/orch.interface';
import type { SelectOption } from '~/components/commons/Settings/Select';

import { X, ChevronUp, ChevronDown } from 'lucide-react';

import ModelCombobox from '~/components/commons/ModelCombobox';
import Select from '~/components/commons/Settings/Select';
import styles from '../styles.module.scss';

const EFFORT_OPTIONS: SelectOption[] = [
  { value: '', label: 'Default' },
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' },
  { value: 'xhigh', label: 'XHigh' },
  { value: 'max', label: 'Max' }
];

interface RoleChainEditorProps {
  role: Role;
  chain: string[];
  efforts: Record<string, Effort>;
  agents: OrchAgent[];
  onChange: (chain: string[]) => void;
  onEffortChange: (entry: string, effort: Effort | null) => void;
}

const RoleChainEditor = ({ role, chain, efforts, agents, onChange, onEffortChange }: RoleChainEditorProps) => {
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
            const agentId = entry.split(':')[0];
            const supportsEffort = agents.find((a) => a.id === agentId)?.supportsEffort ?? false;
            const handleEffort = (val: string) => onEffortChange(entry, val ? (val as Effort) : null);
            const handleMoveUp = () => moveUp(i);
            const handleMoveDown = () => moveDown(i);
            const handleRemove = () => remove(i);
            return (
              <div key={i} className={styles.workerRow}>
                <span className={styles.workerChip}>{entry}</span>
                {supportsEffort && (
                  <div className={styles.effortSelect}>
                    <Select
                      size="sm"
                      options={EFFORT_OPTIONS}
                      value={efforts[entry] ?? ''}
                      onChange={handleEffort}
                    />
                  </div>
                )}
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
