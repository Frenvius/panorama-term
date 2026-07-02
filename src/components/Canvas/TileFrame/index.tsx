import React from 'react';
import { X } from 'lucide-react';

import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import Terminal from '~/components/Terminal';
import { TILE_GAP, TILE_HEADER } from '~/usecase/util/constants';

import styles from './styles.module.scss';

interface TileFrameProps {
  tile: Tile;
  view: View;
  onClose: (id: string) => void;
  onSnap: (id: string) => void;
  onMove: (id: string, dx: number, dy: number) => void;
  onResize: (id: string, dir: string, dx: number, dy: number) => void;
}

const HANDLES = ['n', 's', 'e', 'w', 'nw', 'ne', 'sw', 'se'];

const TileFrame = ({ tile, view, onMove, onSnap, onClose, onResize }: TileFrameProps) => {
  const k = view.k;
  const drag = React.useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null);
  const resize = React.useRef<{ x: number; y: number; dir: string } | null>(null);

  const startDrag = (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    drag.current = { sx: e.clientX, sy: e.clientY, ox: tile.x, oy: tile.y };
  };
  const onDrag = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    onMove(tile.id, d.ox + (e.clientX - d.sx) / k, d.oy + (e.clientY - d.sy) / k);
  };
  const endDrag = () => {
    if (!drag.current) return;
    drag.current = null;
    onSnap(tile.id);
  };

  const startResize = (dir: string) => (e: React.PointerEvent) => {
    e.stopPropagation();
    (e.target as Element).setPointerCapture(e.pointerId);
    resize.current = { x: e.clientX, y: e.clientY, dir };
  };
  const onResizeMove = (e: React.PointerEvent) => {
    const r = resize.current;
    if (!r) return;
    onResize(tile.id, r.dir, e.clientX - r.x, e.clientY - r.y);
    resize.current = { ...r, x: e.clientX, y: e.clientY };
  };
  const endResize = () => {
    if (!resize.current) return;
    resize.current = null;
    onSnap(tile.id);
  };

  const closeTile = () => onClose(tile.id);
  const label = tile.userTitle || tile.autoTitle || `${tile.type} · ${tile.id}`;

  const inset = TILE_GAP / 2;
  const bodyW = tile.width - TILE_GAP;
  const bodyH = tile.height - TILE_GAP;

  return (
    <div
      data-tile
      className={styles.tile}
      style={{
        top: (tile.y + inset) * k + view.y,
        left: (tile.x + inset) * k + view.x,
        width: bodyW,
        height: bodyH,
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
        <button className={styles.close} onClick={closeTile} aria-label="Close tile">
          <X size={14} strokeWidth={2} />
        </button>
      </div>
      <div className={styles.body}>
        {tile.type === 'term' ? (
          <Terminal tileId={tile.id} scale={k} bodyW={bodyW} bodyH={bodyH - TILE_HEADER} />
        ) : (
          <div className={styles.placeholder}>{tile.type}</div>
        )}
      </div>
      {HANDLES.map((dir) => (
        <div
          key={dir}
          data-dir={dir}
          className={styles.handle}
          onPointerUp={endResize}
          onPointerMove={onResizeMove}
          onPointerCancel={endResize}
          onPointerDown={startResize(dir)}
        />
      ))}
    </div>
  );
};

export default TileFrame;
