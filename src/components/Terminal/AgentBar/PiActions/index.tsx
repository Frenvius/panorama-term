import React from 'react';
import { Brain, ChevronDown } from 'lucide-react';

import { aliasLabel } from '~/components/Terminal/AgentBar/parse';
import { EFFORT_LEVELS } from '~/components/Terminal/AgentBar/constants';

import type { AgentModel } from '~/domain/interfaces/pty.interface';

import styles from '~/components/Terminal/AgentBar/styles.module.scss';

interface PiActionsProps {
  effort?: string;
  model?: string;
  efforts?: string[];
  models: AgentModel[];
  contextWindow?: number;
  onPick: { model: (entry: AgentModel) => void; effort: (level: string) => void; context?: (tokens: number) => void };
}

const MAX_LABEL = 22;

const BASE_WINDOW = 200_000;

const shortLabel = (id: string): string => (id.length > MAX_LABEL ? `${id.slice(0, MAX_LABEL - 3)}...` : id);

const effortColor = (level: string): string | undefined => EFFORT_LEVELS.find((l) => l.id === level)?.color;

const windowLabel = (tokens: number): string =>
  tokens >= 1_000_000 ? `${Number((tokens / 1_000_000).toFixed(2))}M` : `${Math.round(tokens / 1000)}k`;

const PiActions = ({ models, model, effort, efforts, contextWindow, onPick }: PiActionsProps) => {
  const [modelMenu, setModelMenu] = React.useState(false);
  const [effortMenu, setEffortMenu] = React.useState(false);
  const [windowMenu, setWindowMenu] = React.useState(false);
  const modelRef = React.useRef<HTMLDivElement>(null);
  const effortRef = React.useRef<HTMLDivElement>(null);
  const windowRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!modelMenu && !effortMenu && !windowMenu) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (!modelRef.current?.contains(target)) setModelMenu(false);
      if (!effortRef.current?.contains(target)) setEffortMenu(false);
      if (!windowRef.current?.contains(target)) setWindowMenu(false);
    };
    document.addEventListener('mousedown', onDown);
    return () => document.removeEventListener('mousedown', onDown);
  }, [modelMenu, effortMenu, windowMenu]);

  const current = models.find((entry) => entry.id === model);
  const levels = current?.efforts ?? efforts ?? [];
  const currentLabel = current ? aliasLabel(current.id, current.provider) : model;
  const maxWindow = current?.contextWindow ?? 0;
  const activeWindow = contextWindow ?? maxWindow;
  const windows = onPick.context && maxWindow > BASE_WINDOW ? [BASE_WINDOW, maxWindow] : [];

  const toggleModelMenu = () => setModelMenu((open) => !open);
  const toggleEffortMenu = () => setEffortMenu((open) => !open);
  const toggleWindowMenu = () => setWindowMenu((open) => !open);

  const pickModel = (entry: AgentModel) => () => {
    onPick.model(entry);
    setModelMenu(false);
  };

  const pickEffort = (level: string) => () => {
    onPick.effort(level);
    setEffortMenu(false);
  };

  const pickWindow = (tokens: number) => () => {
    onPick.context?.(tokens);
    setWindowMenu(false);
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
      {windows.length > 0 && (
        <div className={styles.action} ref={windowRef}>
          <button type="button" className={styles.model} title="Context window" onClick={toggleWindowMenu}>
            {windowLabel(activeWindow)}
            <ChevronDown size={11} />
          </button>
          {windowMenu && (
            <div className={styles.menu}>
              {windows.map((tokens) => (
                <button
                  key={tokens}
                  type="button"
                  onClick={pickWindow(tokens)}
                  className={tokens === activeWindow ? `${styles.menuItem} ${styles.menuActive}` : styles.menuItem}
                >
                  {windowLabel(tokens)}
                  <span className={styles.menuSub}>context</span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      <div className={styles.action} ref={modelRef}>
        <button type="button" className={styles.model} title={model ?? 'Switch model'} onClick={toggleModelMenu}>
          {currentLabel ? shortLabel(currentLabel) : 'Model'}
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
                {aliasLabel(entry.id, entry.provider)}
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
