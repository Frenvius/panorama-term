import React from 'react';

import styles from './styles.module.scss';

interface SliderProps {
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
  formatValue?: (value: number) => string;
  disabled?: boolean;
}

const Slider = ({ min, max, step, value, onChange, formatValue, disabled }: SliderProps) => {
  const pct = `${((value - min) / (max - min)) * 100}%`;
  const display = formatValue ? formatValue(value) : String(value);

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => onChange(Number(e.target.value));

  return (
    <div className={styles.wrapper}>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={handleChange}
        className={styles.track}
        style={{ '--slider-pct': pct } as React.CSSProperties}
      />
      <span className={styles.value}>{display}</span>
    </div>
  );
};

export default Slider;
