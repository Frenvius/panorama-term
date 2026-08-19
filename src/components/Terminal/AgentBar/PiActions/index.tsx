import React from 'react';
import { Brain, ChevronDown } from 'lucide-react';

import { EFFORT_LEVELS } from '~/components/Terminal/AgentBar/constants';

import type { AgentModel } from '~/domain/interfaces/pty.interface';

import styles from '~/components/Terminal/AgentBar/styles.module.scss';

interface PiActionsProps {
  effort?: string;
  model?: string;
  efforts?: string[];
  models: AgentModel[];
  onPick: { model: (entry: AgentModel) => void; effort: (level: string) => void };
}

const MAX_LABEL = 18;

const shortLabel = (id: string): string => (id.length > MAX_LABEL ? `${id.slice(0, MAX_LABEL - 3)}...` : id);

const effortColor = (level: string): string | undefined => EFFORT_LEVELS.find((l) => l.id === level)?.color;

const PiActions = ({ models, model, effort, efforts, onPick }: PiActionsProps) => {
  const [modelMenu, setModelMenu] = React.useState(false);
  const [effortMenu, setEffortMenu] = React.useState(false);
  const modelRef = React.useRef<HTMLDivElement>(null);
  const effortRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!modelMenu && !effortMenu) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!modelRef.current?.contains(target)) setModelMenu(false);
      if (!effortRef.current?.contains(target)) setEffortMenu(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [modelMenu, effortMenu]);

  const current = models.find((entry) => entry.id === model);
  const levels = current?.efforts ?? efforts ?? [];

  const toggleModelMenu = () => setModelMenu((open) => !open);
  const toggleEffortMenu = () => setEffortMenu((open) => !open);

  const pickModel = (entry: AgentModel) => () => {
    onPick.model(entry);
    setModelMenu(false);
  };

  const pickEffort = (level: string) => () => {
    onPick.effort(level);
    setEffortMenu(false);
  };

  if (models.length === 0) return null;

  return (
    <div className={styles.actions}>
      {levels.length > 0 && (
        <div className={styles.action} ref={effortRef}>
          <button
            type="button"
            title={effort ? `Effort: ${effort}` : 'Set effort'}
            className={styles.effort}
            style={effort ? { color: effortColor(effort) } : undefined}
            onClick={toggleEffortMenu}
          >
            <Brain size={14} />
          </button>
          {effortMenu && (
            <div className={styles.menu}>
              {levels.map((level) => (
                <button
                  key={level}
                  type="button"
                  style={{ color: effortColor(level) }}
                  onClick={pickEffort(level)}
                  className={level === effort ? `${styles.menuItem} ${styles.menuActive}` : styles.menuItem}
                >
                  {level}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      <div className={styles.action} ref={modelRef}>
        <button type="button" className={styles.model} title={model ?? 'Switch model'} onClick={toggleModelMenu}>
          {model ? shortLabel(model) : 'Model'}
          <ChevronDown size={11} />
        </button>
        {modelMenu && (
          <div className={`${styles.menu} ${styles.menuScroll}`}>
            {models.map((entry) => (
              <button
                key={`${entry.provider}/${entry.id}`}
                type="button"
                onClick={pickModel(entry)}
                className={entry.id === model ? `${styles.menuItem} ${styles.menuActive}` : styles.menuItem}
              >
                {entry.id}
                <span className={styles.menuSub}>{entry.provider}</span>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

export default PiActions;
