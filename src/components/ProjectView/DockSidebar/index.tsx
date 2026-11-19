import type { DockMode, DockSide, PanelId } from '~/domain/interfaces/dock.interface';
import type { PanelDescriptor } from '~/components/commons/Dock/panels';

import React from 'react';
import { PanelLeftClose, PanelRightClose } from 'lucide-react';

import DockStrip from '~/components/commons/Dock/DockStrip';
import { useDockLayout } from '~/usecase/hooks/useDockLayout';
import styles from './styles.module.scss';

interface DockSidebarProps {
  side: DockSide;
  mode: DockMode;
  width: number;
  panels: PanelDescriptor[];
  onWidth: (w: number) => void;
  onClose: () => void;
  renderPanel: (id: PanelId) => React.ReactNode;
}

const MIN_WIDTH_LEFT = 180;
const MAX_WIDTH_LEFT = 420;
const MIN_WIDTH_RIGHT = 220;
const MAX_WIDTH_RIGHT = 560;

const DockSidebar = ({ side, mode, width, panels, onWidth, onClose, renderPanel }: DockSidebarProps) => {
  const { layout } = useDockLayout(mode);
  const activeId = layout.active[side];

  const minWidth = side === 'left' ? MIN_WIDTH_LEFT : MIN_WIDTH_RIGHT;
  const maxWidth = side === 'left' ? MAX_WIDTH_LEFT : MAX_WIDTH_RIGHT;
  const clampedWidth = Math.min(maxWidth, Math.max(minWidth, width));

  const handleStartResize = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    const startX = e.clientX;
    const startW = clampedWidth;
    el.setPointerCapture(e.pointerId);

    const onMove = (ev: PointerEvent) => {
      const delta = side === 'left' ? ev.clientX - startX : startX - ev.clientX;
      const next = Math.min(maxWidth, Math.max(minWidth, startW + delta));
      onWidth(next);
    };
    const onUp = () => {
      el.removeEventListener('pointermove', onMove);
      el.removeEventListener('pointerup', onUp);
    };
    el.addEventListener('pointermove', onMove);
    el.addEventListener('pointerup', onUp);
  };

  return (
    <aside className={styles.sidebar} style={{ width: clampedWidth }}>
      {side === 'right' && <div className={styles.resizerLeft} onPointerDown={handleStartResize} />}

      <div className={styles.header}>
        <DockStrip mode={mode} side={side} panels={panels} />
        <button
          type="button"
          className={styles.closeBtn}
          onClick={onClose}
          aria-label={side === 'left' ? 'Close left sidebar' : 'Close right sidebar'}
          data-tooltip={side === 'left' ? 'Close left sidebar' : 'Close right sidebar'}
          data-tooltip-place="bottom"
        >
          {side === 'left' ? <PanelLeftClose size={14} strokeWidth={1.75} /> : <PanelRightClose size={14} strokeWidth={1.75} />}
        </button>
      </div>

      <div className={styles.body}>{activeId ? renderPanel(activeId) : null}</div>

      {side === 'left' && <div className={styles.resizerRight} onPointerDown={handleStartResize} />}
    </aside>
  );
};

export default DockSidebar;
