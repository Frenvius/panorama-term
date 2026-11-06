import type { Subagent } from '~/domain/interfaces/pty.interface';

import React from 'react';

import styles from './styles.module.scss';

interface SubagentsProps {
  agents: Subagent[];
  viewing: string;
  onView: (agentId: string) => void;
}

const formatElapsed = (ms: number): string => {
  const total = Math.max(0, Math.floor(ms / 1000));
  const min = Math.floor(total / 60);
  return min ? `${min}m ${total % 60}s` : `${total}s`;
};

const Subagents = ({ agents, viewing, onView }: SubagentsProps) => {
  const [now, setNow] = React.useState(Date.now);

  React.useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  return (
    <div className={styles.list} aria-label="Subagents">
      <button
        type="button"
        className={styles.row}
        onClick={() => onView('')}
        aria-pressed={!viewing}
        title="Main conversation"
      >
        <span className={styles.dot} data-status="main" />
        <span className={styles.type}>main</span>
      </button>
      {agents.map((agent) => (
        <button
          type="button"
          key={agent.id}
          className={styles.row}
          onClick={() => onView(agent.id)}
          aria-pressed={agent.id === viewing}
          title={agent.id === viewing ? 'Back to main conversation' : 'Show transcript'}
        >
          <span className={styles.dot} data-status={agent.status} title={agent.status} />
          <span className={styles.type}>{agent.type}</span>
          <span className={styles.desc}>{agent.description}</span>
          {agent.tool && <span className={styles.tool}>{agent.tool}</span>}
          <span className={styles.elapsed}>{agent.startedAt ? formatElapsed(now - agent.startedAt) : ''}</span>
        </button>
      ))}
    </div>
  );
};

export default Subagents;
