import React from 'react';
import type { Editor } from '@tiptap/react';
import { StickyNote, SquareDashed, SquareTerminal } from 'lucide-react';

import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import Frame from '~/components/Canvas/Frame';
import FrameBar from '~/components/Canvas/FrameBar';
import Minimap from '~/components/Canvas/Minimap';
import TileFrame from '~/components/Canvas/TileFrame';
import NoteToolbar from '~/components/Canvas/NoteToolbar';
import ContextMenu from '~/components/commons/ContextMenu';
import { useCanvas } from '~/usecase/hooks/useCanvas';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { useNotifyBridge } from '~/components/commons/Notifications/bridge';
import { TILE_GAP, CULL_MARGIN, MIN_LIVE_WIDTH } from '~/usecase/util/constants';
import { isCapturing, matchCommand, type CommandId } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

const FS_ANIM = 170;

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
    addNote,
    addTile,
    patchTile,
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
    resetZoom,
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
  const [noteEditors, setNoteEditors] = React.useState<Record<string, Editor>>({});
  const [size, setSize] = React.useState({ w: window.innerWidth, h: window.innerHeight });
  const [fsId, setFsId] = React.useState<string | null>(null);
  const [fsExit, setFsExit] = React.useState(false);
  const fsIdRef = React.useRef<string | null>(fsId);
  fsIdRef.current = fsId;
  const activeTileRef = React.useRef<string | null>(activeTile);
  activeTileRef.current = activeTile;
  const fsTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const activated = React.useRef<Set<string>>(new Set());

  React.useEffect(() => {
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const exitFs = React.useCallback(() => {
    clearTimeout(fsTimer.current);
    setFsExit(true);
    fsTimer.current = setTimeout(() => {
      setFsId(null);
      setFsExit(false);
    }, FS_ANIM);
  }, []);

  const toggleFs = React.useCallback(
    (id: string) => {
      if (fsIdRef.current === id) {
        exitFs();
        return;
      }
      clearTimeout(fsTimer.current);
      setFsExit(false);
      setFsId(id);
    },
    [exitFs]
  );

  React.useEffect(() => () => clearTimeout(fsTimer.current), []);

  React.useEffect(() => {
    const run = (cmd: CommandId): boolean => {
      if (cmd === 'tile.fullscreen') {
        const id = fsIdRef.current ?? activeTileRef.current;
        if (!id) return false;
        toggleFs(id);
        return true;
      }
      if (cmd === 'tile.new') {
        addTile();
        return true;
      }
      if (cmd === 'note.new') {
        addNote();
        return true;
      }
      if (cmd === 'tile.close') {
        const id = activeTileRef.current;
        if (!id) return false;
        closeTile(id);
        return true;
      }
      if (cmd === 'view.resetZoom') {
        resetZoom();
        return true;
      }
      return false;
    };
    const onKey = (e: KeyboardEvent) => {
      if (isCapturing()) return;
      const cmd = matchCommand(e);
      if (!cmd) return;
      if (!run(cmd)) return;
      e.preventDefault();
      e.stopPropagation();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [toggleFs, addTile, addNote, closeTile, resetZoom]);

  React.useEffect(() => {
    if (!fsId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') exitFs();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [fsId, exitFs]);

  React.useEffect(() => {
    if (fsId && !tiles.some((t) => t.id === fsId)) {
      clearTimeout(fsTimer.current);
      setFsId(null);
      setFsExit(false);
    }
  }, [tiles, fsId]);

  const vpW = size.w - 16;
  const vpH = size.h - 54;

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

  const [receded, setReceded] = React.useState<Set<string>>(new Set());

  const focused = tiles.find((t) => t.id === activeTile) ?? null;

  const holds = (f: (typeof frames)[number]): boolean => {
    if (!focused) return false;
    const cx = focused.x + focused.width / 2;
    const cy = focused.y + focused.height / 2;
    return cx >= f.x && cx <= f.x + f.width && cy >= f.y && cy <= f.y + f.height;
  };

  React.useLayoutEffect(() => {
    const bg = bgRef.current;
    if (!bg) return;
    const tileEl = focused && bg.querySelector(`[data-tile="${focused.id}"]`);
    const next = new Set<string>();
    if (tileEl) {
      const tr = tileEl.getBoundingClientRect();
      for (const f of frames) {
        if (holds(f)) continue;
        const barEl = bg.querySelector(`[data-frame-bar="${f.id}"]`);
        if (!barEl) continue;
        const br = barEl.getBoundingClientRect();
        if (br.left < tr.right && br.right > tr.left && br.top < tr.bottom && br.bottom > tr.top) next.add(f.id);
      }
    }
    setReceded((prev) => (prev.size === next.size && [...prev].every((id) => next.has(id)) ? prev : next));
  });

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

  const newNote = () => {
    if (menu) addNote({ x: menu.wx, y: menu.wy });
  };

  const setNoteContent = (id: string, content: string) => patchTile(id, { content });
  const setNoteColor = (id: string, color: string) => patchTile(id, { color });
  const setNoteTitle = (id: string, title: string) => patchTile(id, { userTitle: title });

  const copyNote = (id: string) => {
    const editor = noteEditors[id];
    if (editor) writeClipboard(editor.getText());
  };

  const registerEditor = React.useCallback((id: string, editor: Editor | null) => {
    setNoteEditors((prev) => {
      if (editor) return { ...prev, [id]: editor };
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const activeNote = tiles.find((t) => t.id === activeTile && t.type === 'note') ?? null;
  const activeNoteEditor = activeNote ? noteEditors[activeNote.id] : null;

  const newFrame = () => {
    if (menu) addFrame(menu.wx, menu.wy);
  };

  const openNotified = React.useCallback(
    (id: string) => {
      activateTile(id);
      focusTile(id, true);
    },
    [activateTile, focusTile]
  );

  useNotifyBridge({ tiles, activeTile, onOpen: openNotified });

  return (
    <div className={fsId ? `${styles.root} ${styles.rootFs}` : styles.root}>
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
        {!fsId &&
          frames.map((f) => (
            <Frame key={f.id} frame={f} view={view} onSnap={snapFrame} onResize={resizeFrame} />
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
              onToggleFullscreen={toggleFs}
              onCwd={setTileCwd}
              onNoteChange={setNoteContent}
              onNoteEditor={registerEditor}
              onNoteTitle={setNoteTitle}
              onCopyNote={copyNote}
              active={t.id === activeTile}
              visible={vis}
              live={live}
              fullscreen={t.id === fsId}
              exiting={fsExit}
              hidden={fsId !== null && t.id !== fsId}
              vpW={vpW}
              vpH={vpH}
            />
          );
        })}
        {!fsId &&
          frames.map((f) => (
            <FrameBar
              key={f.id}
              frame={f}
              view={view}
              tiles={tiles}
              recede={receded.has(f.id)}
              onDrag={dragFrame}
              onRemove={removeFrame}
              onRename={renameFrame}
              onRecolor={recolorFrame}
            />
          ))}
        <div ref={indicatorRef} className={styles.indicator}>
          100%
        </div>
        {!fsId && <Minimap view={view} tiles={tiles} viewportRef={bgRef} onPan={panTo} />}
      </div>
      {!fsId && activeNote && activeNoteEditor && (
        <NoteToolbar
          editor={activeNoteEditor}
          color={activeNote.color || '#fef8c4'}
          onColor={(c) => setNoteColor(activeNote.id, c)}
        />
      )}
      {fsId && <div className={fsExit ? `${styles.backdrop} ${styles.backdropExit}` : styles.backdrop} />}
      {menu && (
        <ContextMenu
          x={menu.sx}
          y={menu.sy}
          onClose={closeMenu}
          items={[
            { label: 'New terminal', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: newTerminal },
            { label: 'New note', icon: <StickyNote size={15} strokeWidth={1.75} />, onSelect: newNote },
            { label: 'New frame', icon: <SquareDashed size={15} strokeWidth={1.75} />, onSelect: newFrame }
          ]}
        />
      )}
    </div>
  );
};

export default Canvas;
