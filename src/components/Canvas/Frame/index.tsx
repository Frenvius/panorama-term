import React from 'react';

import type { View, Frame as FrameData } from '~/domain/interfaces/canvas.interface';

import styles from './styles.module.scss';

interface FrameProps {
  frame: FrameData;
  view: View;
  onSnap: (id: string) => void;
  onResize: (id: string, dir: string, dx: number, dy: number) => void;
}

const HANDLES = ['n', 's', 'e', 'w', 'nw', 'ne', 'sw', 'se'];

const Frame = ({ frame, view, onResize, onSnap }: FrameProps) => {
  const k = view.k;
  const resize = React.useRef<{ x: number; y: number; dir: string } | null>(null);

  const startResize = (dir: string) => (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    (e.target as Element).setPointerCapture(e.pointerId);
    resize.current = { x: e.clientX, y: e.clientY, dir };
  };
  const onResizeMove = (e: React.PointerEvent) => {
    const r = resize.current;
    if (!r) return;
    onResize(frame.id, r.dir, e.clientX - r.x, e.clientY - r.y);
    resize.current = { ...r, x: e.clientX, y: e.clientY };
  };
  const endResize = () => {
    if (!resize.current) return;
    resize.current = null;
    onSnap(frame.id);
  };

  return (
    <div
      data-frame={frame.id}
      className={styles.frame}
      style={{
        top: frame.y * k + view.y,
        left: frame.x * k + view.x,
        width: frame.width,
        height: frame.height,
        transform: `scale(${k})`,
        transformOrigin: 'top left',
        ['--frame-color' as string]: frame.color
      }}
    >
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

export default Frame;
