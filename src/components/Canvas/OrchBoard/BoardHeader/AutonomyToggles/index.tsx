import type { Autonomy } from '~/domain/interfaces/orch.interface';

import React from 'react';

import styles from './styles.module.scss';

interface AutonomyTogglesProps {
  autonomy?: Autonomy;
  onChange: (next: Autonomy) => Promise<void>;
}

const TOGGLES: { key: keyof Autonomy; label: string; hint: string }[] = [
  { key: 'operational', label: 'Operational', hint: 'Retry operational failures and resume waiting tasks on their own.' },
  { key: 'execution', label: 'Execution', hint: 'Retry quality failures and start tasks whose dependencies are integrated.' },
  { key: 'integration', label: 'Integration', hint: 'Integrate completed tasks into the run integration branch. Never pushes.' }
];

const ALL_OFF: Autonomy = { operational: false, execution: false, integration: false };

const AutonomyToggles = ({ autonomy, onChange }: AutonomyTogglesProps) => {
  const [pending, setPending] = React.useState<Autonomy | null>(null);
  const pendingRef = React.useRef<Autonomy | null>(null);
  const inFlightRef = React.useRef(0);
  const queueRef = React.useRef<Promise<void>>(Promise.resolve());

  const reconcile = () => {
    if (inFlightRef.current > 0) return;
    pendingRef.current = null;
    setPending(null);
  };

  React.useEffect(reconcile, [autonomy]);

  const toggle = (key: keyof Autonomy) => {
    const base = pendingRef.current ?? autonomy ?? ALL_OFF;
    const next = { ...base, [key]: !base[key] };
    pendingRef.current = next;
    setPending(next);
    inFlightRef.current += 1;
    queueRef.current = queueRef.current
      .then(() => onChange(next))
      .catch(() => {
        pendingRef.current = null;
        setPending(null);
      })
      .finally(() => {
        inFlightRef.current -= 1;
      });
  };

  const shown = pending ?? autonomy;

  return (
    <div className={styles.root} role="group" aria-label="Autonomy">
      <span className={styles.label}>Auto</span>
      {TOGGLES.map(({ key, label, hint }) => {
        const handleClick = () => toggle(key);
        return (
          <button
            key={key}
            type="button"
            role="switch"
            title={hint}
            className={styles.toggle}
            aria-checked={shown?.[key] ?? false}
            onClick={handleClick}
          >
            {label}
          </button>
        );
      })}
    </div>
  );
};

export default AutonomyToggles;
