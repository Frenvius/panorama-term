import React from 'react';
import { Crosshair, ChevronRight } from 'lucide-react';

import type { Frame } from '~/domain/interfaces/canvas.interface';
import type { TileStatus } from '~/usecase/util/tileStatus';

import styles from './styles.module.scss';

export interface FrameGroupHandlers {
  onToggle: (id: string) => void;
  onGoto: (id: string) => void;
}

interface FrameGroupProps {
  frame: Frame;
  open: boolean;
  count: number;
  statuses: TileStatus[];
  handlers: FrameGroupHandlers;
  children: React.ReactNode;
}

const FrameGroup = ({ frame, open, count, statuses, handlers, children }: FrameGroupProps) => {
  const toggle = () => handlers.onToggle(frame.id);
  const goto = (e: React.MouseEvent) => {
    e.stopPropagation();
    handlers.onGoto(frame.id);
  };

  return (
    <div className={styles.group}>
      <div className={styles.header} onClick={toggle} role="button" aria-expanded={open}>
        <ChevronRight size={14} strokeWidth={2} className={styles.caret} data-open={open || undefined} />
        <span className={styles.swatch} style={{ background: frame.color }} />
        <span className={styles.title}>{frame.title}</span>
        {!open && (
          <span className={styles.rollup}>
            {statuses.map((s, i) => (
              <i key={i} data-status={s} />
            ))}
          </span>
        )}
        <span className={styles.count}>{count}</span>
        <button className={styles.goto} onClick={goto} aria-label="Go to frame" data-tooltip="Go to frame">
          <Crosshair size={12} strokeWidth={2} />
        </button>
      </div>
      {open && <div className={styles.body}>{children}</div>}
    </div>
  );
};

export default FrameGroup;
