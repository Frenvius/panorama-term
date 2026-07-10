import React from 'react';
import type { Editor } from '@tiptap/react';
import { X, Copy, RotateCw, Maximize, Minimize } from 'lucide-react';

import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import NoteTile from '~/components/Canvas/NoteTile';
import GridTerminal from '~/components/Terminal/GridTerminal';
import { TILE_GAP, TILE_HEADER } from '~/usecase/util/constants';
import { noteTextColor } from '~/usecase/util/note';

import styles from './styles.module.scss';

interface TileFrameProps {
  tile: Tile;
  view: View;
  active: boolean;
  visible: boolean;
  live: boolean;
  hidden: boolean;
  fullscreen: boolean;
  exiting: boolean;
  vpW: number;
  vpH: number;
  onClose: (id: string) => void;
  onSnap: (id: string) => void;
  onActivate: (id: string) => void;
  onFocusTile: (id: string) => void;
  onToggleFullscreen: (id: string) => void;
  onMove: (id: string, dx: number, dy: number) => void;
  onResize: (id: string, dir: string, dx: number, dy: number) => void;
  onCwd: (id: string, cwd: string) => void;
  onNoteChange: (id: string, content: string) => void;
  onNoteEditor: (id: string, editor: Editor | null) => void;
  onNoteTitle: (id: string, title: string) => void;
  onCopyNote: (id: string) => void;
}

const HANDLES = ['n', 's', 'e', 'w', 'nw', 'ne', 'sw', 'se'];

const FS_PAD = 28;

const devicePx = (v: number): number => {
  const dpr = window.devicePixelRatio || 1;
  return Math.round(v * dpr) / dpr;
};

const TileFrame = ({ tile, view, active, visible, live, hidden, fullscreen, exiting, vpW, vpH, onMove, onSnap, onClose, onResize, onActivate, onFocusTile, onToggleFullscreen, onCwd, onNoteChange, onNoteEditor, onNoteTitle, onCopyNote }: TileFrameProps) => {
  const k = view.k;
  const drag = React.useRef<{ sx: number; sy: number; ox: number; oy: number } | null>(null);
  const resize = React.useRef<{ x: number; y: number; dir: string } | null>(null);

  const startDrag = (e: React.PointerEvent) => {
    if (e.button !== 0 || fullscreen) return;
    e.stopPropagation();
    onActivate(tile.id);
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
    if (e.button !== 0) return;
    e.stopPropagation();
    onActivate(tile.id);
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

  const [restartKey, setRestartKey] = React.useState(0);
  const restartTile = () => setRestartKey((n) => n + 1);
  const closeTile = () => onClose(tile.id);
  const focusTile = () => onFocusTile(tile.id);
  const toggleFullscreen = () => onToggleFullscreen(tile.id);
  const label = tile.userTitle || tile.autoTitle || `${tile.type} · ${tile.id}`;

  const note = tile.type === 'note';
  const noteTint = note ? { background: tile.color, color: noteTextColor(tile.color || '#fef8c4') } : null;
  const copyNote = () => onCopyNote(tile.id);
  const changeTitle = (e: React.ChangeEvent<HTMLInputElement>) => onNoteTitle(tile.id, e.target.value);
  const stopDrag = (e: React.PointerEvent) => e.stopPropagation();

  const inset = TILE_GAP / 2;
  const ek = fullscreen ? 1 : k;
  const bodyW = fullscreen ? vpW - FS_PAD * 2 : tile.width - TILE_GAP;
  const bodyH = fullscreen ? vpH - FS_PAD * 2 : tile.height - TILE_GAP;
  const sx = fullscreen ? FS_PAD : (tile.x + inset) * k + view.x;
  const sy = fullscreen ? FS_PAD : (tile.y + inset) * k + view.y;
  const z = fullscreen ? 50 : active ? 2 : 1;
  const box = fullscreen
    ? { width: bodyW, height: bodyH }
    : { width: bodyW, height: bodyH, transform: `scale(${k})`, transformOrigin: 'top left' as const };
  const term = tile.type === 'term' && live;
  const anim = fullscreen ? (exiting ? styles.fsExit : styles.fsEnter) : null;
  const cls = [styles.tile, note && styles.sticky, active && !fullscreen && styles.active, anim].filter(Boolean).join(' ');
  const gone = { display: hidden ? 'none' : undefined };

  return (
    <>
      <div data-tile={tile.id} className={cls} style={{ top: sy, left: sx, zIndex: z, ...box, ...gone, ...noteTint }}>
        <div
          className={styles.header}
          onPointerUp={endDrag}
          onPointerDown={startDrag}
          onPointerMove={onDrag}
          onPointerCancel={endDrag}
          onDoubleClick={focusTile}
        >
          {note ? (
            <input
              className={styles.noteTitle}
              value={tile.userTitle ?? ''}
              placeholder="Note"
              onChange={changeTitle}
              onPointerDown={stopDrag}
            />
          ) : (
            <span className={styles.title}>{label}</span>
          )}
          <div className={styles.actions}>
            {note && (
              <button className={styles.action} onClick={copyNote} aria-label="Copy note">
                <Copy size={13} strokeWidth={2} />
              </button>
            )}
            <button className={styles.action} onClick={toggleFullscreen} aria-label="Toggle fullscreen">
              {fullscreen ? <Minimize size={13} strokeWidth={2} /> : <Maximize size={13} strokeWidth={2} />}
            </button>
            {!note && (
              <button className={styles.action} onClick={restartTile} aria-label="Restart terminal">
                <RotateCw size={13} strokeWidth={2} />
              </button>
            )}
            {!fullscreen && (
              <button className={`${styles.action} ${styles.close}`} onClick={closeTile} aria-label="Close tile">
                <X size={14} strokeWidth={2} />
              </button>
            )}
          </div>
        </div>
        <div className={styles.body}>
          {note ? (
            <NoteTile tile={tile} onChange={onNoteChange} onActivate={onActivate} onEditor={onNoteEditor} />
          ) : (
            !term && <div className={styles.placeholder}>{tile.type !== 'term' ? label : ''}</div>
          )}
        </div>
      </div>
      {term && (
        <div
          data-tile={tile.id}
          className={anim ? `${styles.termLayer} ${anim}` : styles.termLayer}
          style={{
            top: devicePx(sy + (TILE_HEADER + 4) * ek),
            left: devicePx(sx + 4 * ek),
            width: devicePx((bodyW - 8) * ek),
            height: devicePx((bodyH - TILE_HEADER - 5) * ek),
            zIndex: z,
            ...gone
          }}
        >
          <GridTerminal
            k={ek}
            cwd={tile.cwd}
            onCwd={onCwd}
            restartKey={restartKey}
            active={active}
            visible={visible && !hidden}
            tileId={tile.id}
            cols={Math.max(20, Math.floor((bodyW - 8) / 7.23))}
            rows={Math.max(2, Math.floor((bodyH - TILE_HEADER - 4) / 15))}
          />
        </div>
      )}
      {!fullscreen && (
        <div data-tile={tile.id} className={styles.handles} style={{ top: sy, left: sx, zIndex: z, ...box, ...gone }}>
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
      )}
    </>
  );
};

export default TileFrame;
