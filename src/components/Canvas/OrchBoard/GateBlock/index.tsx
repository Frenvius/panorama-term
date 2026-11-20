import type { Gate } from '~/domain/interfaces/orch.interface';

import React from 'react';

import styles from './styles.module.scss';

interface GateBlockProps {
  gate: Gate;
  resolving: string | null;
  onResolve: (gateId: string, resolution: string) => void;
}

const GateBlock: React.FC<GateBlockProps> = ({ gate, resolving, onResolve }) => (
  <div className={styles.gateCard}>
    <div className={styles.gateQuestion}>{gate.question}</div>
    <div className={styles.gateOptions}>
      {gate.options.map((opt) => {
        const handleClick = () => onResolve(gate.id, opt);
        return (
          <button
            key={opt}
            type="button"
            className={styles.gateBtn}
            disabled={resolving === gate.id}
            onClick={handleClick}
          >
            {opt}
          </button>
        );
      })}
    </div>
  </div>
);

export default GateBlock;
