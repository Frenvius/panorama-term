import React from 'react';

import styles from './styles.module.scss';

interface SplitHandleProps {
  left: number;
  onMove: (clientX: number) => void;
}

const SplitHandle = ({ left, onMove }: SplitHandleProps) => {
  const startDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const move = (ev: PointerEvent) => onMove(ev.clientX);
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };

  return <div className={styles.handle} style={{ left }} onPointerDown={startDrag} />;
};

export default SplitHandle;
