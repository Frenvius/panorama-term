import type { DockMode, DockSide, PanelId } from '~/domain/interfaces/dock.interface';
import type { PanelDescriptor } from '../panels';

import React from 'react';

import ContextMenu from '~/components/commons/ContextMenu';
import { useDockLayout } from '~/usecase/hooks/useDockLayout';
import { buildIconMenu, buildStripMenu } from '../menuItems';
import styles from './styles.module.scss';

interface DockStripProps {
  mode: DockMode;
  side: DockSide;
  panels: PanelDescriptor[];
}

interface IconMenu {
  x: number;
  y: number;
  id: PanelId;
}

interface StripMenu {
  x: number;
  y: number;
}

const DockStrip = ({ mode, side, panels }: DockStripProps) => {
  const { layout, move, hide, show, reset, setActive } = useDockLayout(mode);
  const [iconMenu, setIconMenu] = React.useState<IconMenu | null>(null);
  const [stripMenu, setStripMenu] = React.useState<StripMenu | null>(null);

  const activeId = layout.active[side];

  const closeIconMenu = () => setIconMenu(null);
  const closeStripMenu = () => setStripMenu(null);

  const handleStripContext = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest('button')) return;
    e.preventDefault();
    setStripMenu({ x: e.clientX, y: e.clientY });
  };

  return (
    <div className={styles.strip} onContextMenu={handleStripContext}>
      {panels.map((panel) => {
        const handleClick = () => setActive(side, panel.id);
        const handleContext = (e: React.MouseEvent) => {
          e.preventDefault();
          e.stopPropagation();
          setIconMenu({ x: e.clientX, y: e.clientY, id: panel.id });
        };
        return (
          <button
            key={panel.id}
            type="button"
            className={styles.tab}
            onClick={handleClick}
            onContextMenu={handleContext}
            aria-label={panel.label}
            data-tooltip={panel.label}
            data-tooltip-place="bottom"
            data-active={activeId === panel.id || undefined}
          >
            <panel.icon size={14} strokeWidth={2} />
          </button>
        );
      })}

      {iconMenu && (
        <ContextMenu x={iconMenu.x} y={iconMenu.y} onClose={closeIconMenu} items={buildIconMenu(iconMenu.id, side, move, hide)} />
      )}

      {stripMenu && (
        <ContextMenu
          x={stripMenu.x}
          y={stripMenu.y}
          onClose={closeStripMenu}
          items={buildStripMenu({ mode, layout, show, hide, reset })}
        />
      )}
    </div>
  );
};

export default DockStrip;
