import type { RunSummary } from '~/domain/interfaces/orch.interface';
import type { FrameArrangement } from '~/usecase/util/reorderLayout';
import type { Tile, View, Frame as FrameData, FrameMember } from '~/domain/interfaces/canvas.interface';

import React from 'react';
import { X, Lock, Rows2, Trash2, Shrink, Grid2x2, Grid3x3, Palette, PenLine, Columns2, LayoutGrid, LockOpen, SquareKanban, WandSparkles } from 'lucide-react';

import ContextMenu from '~/components/commons/ContextMenu';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import { boardSubmenu } from '~/components/commons/ContextMenu/boardSubmenu';

import styles from '~/components/Canvas/Frame/styles.module.scss';

export interface FrameBoardProp {
  runs: RunSummary[];
  onOpen: (runId: string, frameId?: string) => void;
  onNew: () => void;
  onSetFolder: () => void;
}

export interface FrameBarActions {
  onFit: (id: string) => void;
  onDrag: (id: string, x: number, y: number, members: FrameMember[]) => void;
  onRemove: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onRecolor: (id: string, color: string) => void;
  onDragEnd: (id: string, memberIds: string[]) => void;
  onReorder: (id: string, arrangement: FrameArrangement) => void;
  onRemoveWithTiles: (id: string) => void;
  onToggleOrderLock: (id: string) => void;
}

interface FrameBarProps {
  frame: FrameData;
  view: View;
  tiles: Tile[];
  recede: boolean;
  board?: FrameBoardProp;
  actions: FrameBarActions;
}

interface DragState {
  sx: number;
  sy: number;
  fx: number;
  fy: number;
  members: { id: string; sx: number; sy: number }[];
}

const inside = (f: FrameData, t: Tile): boolean => {
  const cx = t.x + t.width / 2;
  const cy = t.y + t.height / 2;
  return cx >= f.x && cx <= f.x + f.width && cy >= f.y && cy <= f.y + f.height;
};

const ARRANGEMENTS: { arrangement: FrameArrangement; label: string; icon: React.ReactNode }[] = [
  { arrangement: 'auto', label: 'Auto', icon: <WandSparkles size={15} strokeWidth={1.75} /> },
  { arrangement: 'horizontal', label: 'Horizontal', icon: <Columns2 size={15} strokeWidth={1.75} /> },
  { arrangement: 'vertical', label: 'Vertical', icon: <Rows2 size={15} strokeWidth={1.75} /> },
  { arrangement: 'grid2', label: 'Grid 2x2', icon: <Grid2x2 size={15} strokeWidth={1.75} /> },
  { arrangement: 'grid3', label: 'Grid 3x3', icon: <Grid3x3 size={15} strokeWidth={1.75} /> }
];

const FrameBar = ({ frame, view, tiles, recede, board, actions }: FrameBarProps) => {
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
    actions.onDrag(frame.id, d.fx + dx, d.fy + dy, d.members.map((m) => ({ id: m.id, x: m.sx + dx, y: m.sy + dy })));
  };
  const endDrag = () => {
    const d = drag.current;
    if (!d) return;
    drag.current = null;
    actions.onDragEnd(frame.id, d.members.map((m) => m.id));
  };

  const stop = (e: React.PointerEvent) => e.stopPropagation();
  const openColor = () => colorRef.current?.click();
  const recolor = (e: React.ChangeEvent<HTMLInputElement>) => actions.onRecolor(frame.id, e.currentTarget.value);
  const remove = () => actions.onRemove(frame.id);
  const removeWithTiles = () => actions.onRemoveWithTiles(frame.id);
  const fit = () => actions.onFit(frame.id);
  const toggleOrderLock = () => actions.onToggleOrderLock(frame.id);

  const reorderEntries: ContextMenuEntry[] = ARRANGEMENTS.map(({ arrangement, label, icon }) => ({
    label,
    icon,
    onSelect: () => actions.onReorder(frame.id, arrangement)
  }));

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
    actions.onRename(frame.id, (e.currentTarget as HTMLInputElement).value.trim() || 'Frame');
    setEditing(false);
  };
  const onRenameKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === 'Enter') commitRename(e);
    if (e.key === 'Escape') setEditing(false);
  };

  return (
    <div
      data-frame={frame.id}
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
          <span className={styles.title}>
            {frame.orderLocked && <Lock size={11} strokeWidth={1.75} className={styles.lockIcon} />}
            {frame.title}
          </span>
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
            { label: 'Reorder', icon: <LayoutGrid size={15} strokeWidth={1.75} />, submenu: reorderEntries },
            { label: frame.orderLocked ? 'Unlock order' : 'Lock order', icon: frame.orderLocked ? <LockOpen size={15} strokeWidth={1.75} /> : <Lock size={15} strokeWidth={1.75} />, onSelect: toggleOrderLock },
            ...(board
              ? ([
                  'separator',
                  {
                    label: 'Board',
                    icon: <SquareKanban size={15} strokeWidth={1.75} />,
                    submenu: frame.path
                      ? boardSubmenu(board.runs, (runId) => board.onOpen(runId, frame.id), board.onNew, frame.path)
                      : [...board.runs.map((s) => ({ label: `${s.run.title} (${s.run.repo.replace(/[\\/]+$/, '').split(/[\\/]/).pop()})`, onSelect: () => board.onOpen(s.run.id, frame.id) })), { label: 'Set folder...', onSelect: board.onSetFolder }]
                  }
                ] as ContextMenuEntry[])
              : []),
            'separator',
            { label: 'Delete frame', icon: <X size={15} strokeWidth={1.75} />, onSelect: remove },
            { label: 'Delete frame and tiles', icon: <Trash2 size={15} strokeWidth={1.75} />, danger: true, onSelect: removeWithTiles }
          ] as ContextMenuEntry[]}
        />
      )}
    </div>
  );
};

export default FrameBar;
