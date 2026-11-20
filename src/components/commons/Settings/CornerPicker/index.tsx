import type { NotifCorner } from '~/usecase/util/notifPlacement';

import React from 'react';

import { NOTIF_CORNERS } from '~/usecase/util/notifPlacement';

import styles from './styles.module.scss';

interface CornerPickerProps {
  value: NotifCorner;
  onChange: (corner: NotifCorner) => void;
  label?: string;
}

const CORNER_CLASS: Record<NotifCorner, string> = {
  'top-left': styles.topLeft,
  'top-right': styles.topRight,
  'bottom-left': styles.bottomLeft,
  'bottom-right': styles.bottomRight
};

const CornerPicker = ({ value, onChange, label }: CornerPickerProps) => {
  const handleKeyDown = (e: React.KeyboardEvent, idx: number) => {
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') {
      e.preventDefault();
      onChange(NOTIF_CORNERS[(idx + 1) % NOTIF_CORNERS.length].id);
    } else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') {
      e.preventDefault();
      onChange(NOTIF_CORNERS[(idx - 1 + NOTIF_CORNERS.length) % NOTIF_CORNERS.length].id);
    }
  };

  const currentLabel = NOTIF_CORNERS.find((c) => c.id === value)?.label ?? '';

  return (
    <div className={styles.wrapper}>
      <div role="radiogroup" aria-label={label ?? 'Corner'} className={styles.screen}>
        {NOTIF_CORNERS.map(({ id, label: cornerLabel }, idx) => {
          const handleClick = () => onChange(id);
          const handleKey = (e: React.KeyboardEvent) => handleKeyDown(e, idx);
          return (
            <button
              key={id}
              type="button"
              role="radio"
              aria-checked={value === id}
              aria-label={cornerLabel}
              onClick={handleClick}
              onKeyDown={handleKey}
              className={`${styles.target} ${CORNER_CLASS[id]} ${value === id ? styles.selected : ''}`}
            />
          );
        })}
      </div>
      <span className={styles.currentLabel}>{currentLabel}</span>
    </div>
  );
};

export default CornerPicker;
