import React from 'react';
import { SquareTerminal } from 'lucide-react';

import TileFrame from '~/components/Canvas/TileFrame';
import ContextMenu from '~/components/commons/ContextMenu';
import { useCanvas } from '~/usecase/hooks/useCanvas';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';

import styles from './styles.module.scss';

interface Menu {
  sx: number;
  sy: number;
  wx: number;
  wy: number;
}

const Canvas = () => {
  const { activeState, saveActiveState } = useWorkspace();
  const {
    view,
    tiles,
    bgRef,
    endPan,
    addTile,
    gridRef,
    onWheel,
    moveTile,
    snapTile,
    closeTile,
    resizeTile,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  } = useCanvas({ seed: activeState, onPersist: saveActiveState });

  const [menu, setMenu] = React.useState<Menu | null>(null);

  const closeMenu = () => setMenu(null);

  const openMenu = (e: React.MouseEvent) => {
    if ((e.target as Element).closest('[data-tile]')) return;
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    const wx = (e.clientX - rect.left - view.x) / view.k;
    const wy = (e.clientY - rect.top - view.y) / view.k;
    setMenu({ sx: e.clientX, sy: e.clientY, wx, wy });
  };

  const newTerminal = () => {
    if (menu) addTile({ x: menu.wx, y: menu.wy });
  };

  return (
    <div className={styles.root}>
      <div
        ref={bgRef}
        className={styles.bg}
        onWheel={onWheel}
        onPointerUp={endPan}
        onContextMenu={openMenu}
        onPointerDown={onBgPointerDown}
        onPointerMove={onBgPointerMove}
        onPointerCancel={endPan}
      >
        <canvas ref={gridRef} className={styles.grid} />
        {tiles.map((t) => (
          <TileFrame
            key={t.id}
            tile={t}
            view={view}
            onMove={moveTile}
            onSnap={snapTile}
            onClose={closeTile}
            onResize={resizeTile}
          />
        ))}
        <div ref={indicatorRef} className={styles.indicator}>
          100%
        </div>
      </div>
      {menu && (
        <ContextMenu
          x={menu.sx}
          y={menu.sy}
          onClose={closeMenu}
          items={[{ label: 'New terminal', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: newTerminal }]}
        />
      )}
    </div>
  );
};

export default Canvas;
