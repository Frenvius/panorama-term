import TileFrame from '~/components/Canvas/TileFrame';
import { useCanvas } from '~/usecase/hooks/useCanvas';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';

import styles from './styles.module.scss';

const Canvas = () => {
  const { activeState, saveActiveState } = useWorkspace();
  const {
    view,
    tiles,
    bgRef,
    endPan,
    gridRef,
    onWheel,
    moveTile,
    closeTile,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  } = useCanvas({ seed: activeState, onPersist: saveActiveState });

  return (
    <div className={styles.root}>
      <div
        ref={bgRef}
        className={styles.bg}
        onWheel={onWheel}
        onPointerUp={endPan}
        onPointerDown={onBgPointerDown}
        onPointerMove={onBgPointerMove}
        onPointerCancel={endPan}
      >
        <canvas ref={gridRef} className={styles.grid} />
        {tiles.map((t) => (
          <TileFrame key={t.id} tile={t} view={view} onMove={moveTile} onClose={closeTile} />
        ))}
        <div ref={indicatorRef} className={styles.indicator}>
          100%
        </div>
      </div>
    </div>
  );
};

export default Canvas;
