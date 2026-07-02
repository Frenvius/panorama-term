import React from 'react';

import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import Terminal from '~/components/Terminal';
import { TILE_HEADER } from '~/usecase/util/constants';

import styles from './styles.module.scss';

interface TileFrameProps {
  tile: Tile;
  view: View;
  onClose: (id: string) => void;
  onMove: (id: string, dx: number, dy: number) => void;
}

const TileFrame = ({ tile, view, onMove, onClose }: TileFrameProps) => {
  const k = view.k;
  const drag = React.useRef<{ x: number; y: number } | null>(null);

  const startDrag = (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, y: e.clientY };
  };
  const onDrag = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    onMove(tile.id, e.clientX - d.x, e.clientY - d.y);
    drag.current = { x: e.clientX, y: e.clientY };
  };
  const endDrag = () => {
    drag.current = null;
  };
  const closeTile = () => onClose(tile.id);
  const label = tile.userTitle || tile.autoTitle || `${tile.type} · ${tile.id}`;

  return (
    <div
      data-tile
      className={styles.tile}
      style={{
        top: tile.y * k + view.y,
        left: tile.x * k + view.x,
        width: tile.width,
        height: tile.height,
        transform: `scale(${k})`,
        transformOrigin: 'top left'
      }}
    >
      <div
        className={styles.header}
        onPointerUp={endDrag}
        onPointerDown={startDrag}
        onPointerMove={onDrag}
        onPointerCancel={endDrag}
      >
        <span className={styles.title}>{label}</span>
        <button className={styles.close} onClick={closeTile}>
          ×
        </button>
      </div>
      <div className={styles.body}>
        {tile.type === 'term' ? (
          <Terminal tileId={tile.id} scale={k} bodyW={tile.width} bodyH={tile.height - TILE_HEADER} />
        ) : (
          <div className={styles.placeholder}>{tile.type}</div>
        )}
      </div>
    </div>
  );
};

export default TileFrame;
