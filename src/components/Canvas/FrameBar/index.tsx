import React from 'react';
import { X, Trash2, Shrink, Palette, PenLine } from 'lucide-react';

import type { Tile, View, Frame as FrameData, FrameMember } from '~/domain/interfaces/canvas.interface';
import ContextMenu from '~/components/commons/ContextMenu';
import { CELL } from '~/usecase/util/constants';

import styles from '~/components/Canvas/Frame/styles.module.scss';

interface FrameBarProps {
  frame: FrameData;
  view: View;
  tiles: Tile[];
  recede: boolean;
  onFit: (id: string) => void;
  onSnap: (id: string) => void;
  onRemove: (id: string) => void;
  onRemoveWithTiles: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onRecolor: (id: string, color: string) => void;
  onDrag: (id: string, x: number, y: number, members: FrameMember[]) => void;
}

interface DragState {
  sx: number;
  sy: number;
  fx: number;
  fy: number;
  members: { id: string; sx: number; sy: number }[];
}

const snap = (v: number): number => Math.round(v / CELL) * CELL;

const inside = (f: FrameData, t: Tile): boolean => {
  const cx = t.x + t.width / 2;
  const cy = t.y + t.height / 2;
  return cx >= f.x && cx <= f.x + f.width && cy >= f.y && cy <= f.y + f.height;
};

const FrameBar = ({ frame, view, tiles, recede, onFit, onDrag, onSnap, onRename, onRecolor, onRemove, onRemoveWithTiles }: FrameBarProps) => {
  const k = view.k;
  const [editing, setEditing] = React.useState(false);
  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);
  const colorRef = React.useRef<HTMLInputElement>(null);
  const drag = React.useRef<DragState | null>(null);

  const startDrag = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    e.stopPropagation();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    drag.current = {
      sx: e.clientX,
      sy: e.clientY,
      fx: frame.x,
      fy: frame.y,
      members: tiles.filter((t) => inside(frame, t)).map((t) => ({ id: t.id, sx: t.x, sy: t.y }))
    };
  };
  const onDragMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = (e.clientX - d.sx) / k;
    const dy = (e.clientY - d.sy) / k;
    onDrag(frame.id, d.fx + dx, d.fy + dy, d.members.map((m) => ({ id: m.id, x: m.sx + dx, y: m.sy + dy })));
  };
  const endDrag = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    const dx = (e.clientX - d.sx) / k;
    const dy = (e.clientY - d.sy) / k;
    onDrag(frame.id, snap(d.fx + dx), snap(d.fy + dy), d.members.map((m) => ({ id: m.id, x: snap(m.sx + dx), y: snap(m.sy + dy) })));
    onSnap(frame.id);
  };

  const stop = (e: React.PointerEvent) => e.stopPropagation();
  const openColor = () => colorRef.current?.click();
  const recolor = (e: React.ChangeEvent<HTMLInputElement>) => onRecolor(frame.id, e.currentTarget.value);
  const remove = () => onRemove(frame.id);
  const removeWithTiles = () => onRemoveWithTiles(frame.id);
  const fit = () => onFit(frame.id);

  const startRename = () => setEditing(true);
  const onBarDoubleClick = (e: React.MouseEvent) => {
    if ((e.target as Element).closest('button, input')) return;
    setEditing(true);
  };

  const closeMenu = () => setMenu(null);
  const openMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    drag.current = null;
    setMenu({ x: e.clientX, y: e.clientY });
  };
  const commitRename = (e: React.FocusEvent<HTMLInputElement> | React.KeyboardEvent<HTMLInputElement>) => {
    onRename(frame.id, (e.currentTarget as HTMLInputElement).value.trim() || 'Frame');
    setEditing(false);
  };
  const onRenameKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === 'Enter') commitRename(e);
    if (e.key === 'Escape') setEditing(false);
  };

  return (
    <div
      className={styles.barLayer}
      style={{
        top: frame.y * k + view.y,
        left: frame.x * k + view.x,
        width: frame.width,
        height: frame.height,
        transform: `scale(${k})`,
        transformOrigin: 'top left',
        zIndex: recede ? 0 : 4,
        ['--frame-color' as string]: frame.color
      }}
    >
      <div
        data-frame-bar={frame.id}
        className={styles.bar}
        onPointerUp={endDrag}
        onPointerDown={startDrag}
        onPointerMove={onDragMove}
        onPointerCancel={endDrag}
        onContextMenu={openMenu}
        onDoubleClick={onBarDoubleClick}
      >
        {editing ? (
          <input
            ref={(el) => el?.focus({ preventScroll: true })}
            type="text"
            defaultValue={frame.title}
            className={styles.renameInput}
            onBlur={commitRename}
            onKeyDown={onRenameKey}
            onPointerDown={stop}
          />
        ) : (
          <span className={styles.title}>{frame.title}</span>
        )}
        <button className={styles.colorBtn} onClick={openColor} onPointerDown={stop} aria-label="Frame color" />
        <input ref={colorRef} type="color" value={frame.color} className={styles.colorInput} onChange={recolor} />
        <button className={styles.delBtn} onClick={remove} onPointerDown={stop} aria-label="Delete frame">
          <X size={14} strokeWidth={2} />
        </button>
      </div>
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={closeMenu}
          items={[
            { label: 'Rename', icon: <PenLine size={15} strokeWidth={1.75} />, onSelect: startRename },
            { label: 'Change color', icon: <Palette size={15} strokeWidth={1.75} />, onSelect: openColor },
            { label: 'Fit to contents', icon: <Shrink size={15} strokeWidth={1.75} />, onSelect: fit },
            'separator',
            { label: 'Delete frame', icon: <X size={15} strokeWidth={1.75} />, onSelect: remove },
            { label: 'Delete frame and tiles', icon: <Trash2 size={15} strokeWidth={1.75} />, danger: true, onSelect: removeWithTiles }
          ]}
        />
      )}
    </div>
  );
};

export default FrameBar;
