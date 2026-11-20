import React from 'react';

import styles from './styles.module.scss';

interface SegmentedOption<T extends string> {
  value: T;
  label: string;
}

interface SegmentedProps<T extends string> {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  label?: string;
}

const Segmented = <T extends string,>({ options, value, onChange, label }: SegmentedProps<T>) => {
  const handleKeyDown = (e: React.KeyboardEvent, idx: number) => {
    if (e.key === 'ArrowRight') {
      e.preventDefault();
      onChange(options[(idx + 1) % options.length].value);
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      onChange(options[(idx - 1 + options.length) % options.length].value);
    }
  };

  return (
    <div role="radiogroup" aria-label={label} className={styles.segmented}>
      {options.map((opt, idx) => {
        const handleClick = () => onChange(opt.value);
        const handleKey = (e: React.KeyboardEvent) => handleKeyDown(e, idx);
        return (
          <button
            key={opt.value}
            type="button"
            role="radio"
            aria-checked={value === opt.value}
            onClick={handleClick}
            onKeyDown={handleKey}
            className={`${styles.btn} ${value === opt.value ? styles.active : ''}`}
          >
            {opt.label}
          </button>
        );
      })}
    </div>
  );
};

export default Segmented;
