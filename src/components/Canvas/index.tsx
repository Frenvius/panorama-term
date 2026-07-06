import React from 'react';
import { SquareDashed, SquareTerminal } from 'lucide-react';

import Frame from '~/components/Canvas/Frame';
import Minimap from '~/components/Canvas/Minimap';
import TileFrame from '~/components/Canvas/TileFrame';
import ContextMenu from '~/components/commons/ContextMenu';
import { useCanvas } from '~/usecase/hooks/useCanvas';
import { TILE_GAP, CULL_MARGIN, MIN_LIVE_WIDTH } from '~/usecase/util/constants';
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
    panTo,
    bgRef,
    frames,
    endPan,
    addTile,
    addFrame,
    gridRef,
    focusTile,
    onWheel,
    moveTile,
    snapTile,
    dragFrame,
    closeTile,
    snapFrame,
    activeTile,
    resizeTile,
    removeFrame,
    renameFrame,
    resizeFrame,
    recolorFrame,
    activateTile,
    setTileCwd,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  } = useCanvas({ seed: activeState, onPersist: saveActiveState });

  const [menu, setMenu] = React.useState<Menu | null>(null);
  const [size, setSize] = React.useState({ w: window.innerWidth, h: window.innerHeight });
  const activated = React.useRef<Set<string>>(new Set());

  React.useEffect(() => {
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const inset = TILE_GAP / 2;
  const isVisible = (t: (typeof tiles)[number]): boolean => {
    const left = (t.x + inset) * view.k + view.x;
    const top = (t.y + inset) * view.k + view.y;
    const w = (t.width - TILE_GAP) * view.k;
    const h = (t.height - TILE_GAP) * view.k;
    return (
      left < size.w + CULL_MARGIN &&
      left + w > -CULL_MARGIN &&
      top < size.h + CULL_MARGIN &&
      top + h > -CULL_MARGIN
    );
  };

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

  const newFrame = () => {
    if (menu) addFrame(menu.wx, menu.wy);
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
        {frames.map((f) => (
          <Frame
            key={f.id}
            frame={f}
            view={view}
            tiles={tiles}
            onDrag={dragFrame}
            onSnap={snapFrame}
            onResize={resizeFrame}
            onRemove={removeFrame}
            onRename={renameFrame}
            onRecolor={recolorFrame}
          />
        ))}
        {tiles.map((t) => {
          const vis = isVisible(t);
          if (vis && (t.width - TILE_GAP) * view.k >= MIN_LIVE_WIDTH) activated.current.add(t.id);
          const live = activated.current.has(t.id);
          return (
            <TileFrame
              key={t.id}
              tile={t}
              view={view}
              onMove={moveTile}
              onSnap={snapTile}
              onClose={closeTile}
              onResize={resizeTile}
              onActivate={activateTile}
              onFocusTile={focusTile}
              onCwd={setTileCwd}
              active={t.id === activeTile}
              visible={vis}
              live={live}
            />
          );
        })}
        <div ref={indicatorRef} className={styles.indicator}>
          100%
        </div>
        <Minimap view={view} tiles={tiles} viewportRef={bgRef} onPan={panTo} />
      </div>
      {menu && (
        <ContextMenu
          x={menu.sx}
          y={menu.sy}
          onClose={closeMenu}
          items={[
            { label: 'New terminal', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: newTerminal },
            { label: 'New frame', icon: <SquareDashed size={15} strokeWidth={1.75} />, onSelect: newFrame }
          ]}
        />
      )}
    </div>
  );
};

export default Canvas;
