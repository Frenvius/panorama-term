import React from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { EditorView } from '@codemirror/view';
import { Group, StickyNote, SquareDashed, SquareTerminal } from 'lucide-react';

import { revealPath } from '~/adapter/shell/shell.client';
import { readClipboard, writeClipboard } from '~/adapter/clipboard/clipboard.client';
import Frame from '~/components/Canvas/Frame';
import FrameBar from '~/components/Canvas/FrameBar';
import Minimap from '~/components/Canvas/Minimap';
import Palette from '~/components/Canvas/Palette';
import TileFrame from '~/components/Canvas/TileFrame';
import Navigator from '~/components/Canvas/Navigator';
import DiffViewer from '~/components/DiffViewer';
import NoteToolbar from '~/components/Canvas/NoteToolbar';
import ContextMenu from '~/components/commons/ContextMenu';
import { useCanvas } from '~/usecase/hooks/useCanvas';
import { writeNote } from '~/adapter/notes/notes.client';
import { applyFrontTitle } from '~/usecase/util/noteMeta';
import { adjacentTerm, termName, flowPath } from '~/usecase/util/noteLink';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { useNotifyBridge, type NotifyKind } from '~/components/commons/Notifications/bridge';
import { TILE_GAP, CULL_MARGIN, MIN_LIVE_WIDTH } from '~/usecase/util/constants';
import { isCapturing, getBinding, formatCombo, matchCommand, type CommandId } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

const FS_ANIM = 170;
const DIFF_ANIM = 130;

let pendingFocusTileId: string | null = null;

const DBLCLICK_MS = 400;
const ALERTS_KEY = 'panorama:alerts';

const loadAlerts = (): Map<string, NotifyKind> => {
  try {
    const raw = localStorage.getItem(ALERTS_KEY);
    return raw ? new Map(Object.entries(JSON.parse(raw) as Record<string, NotifyKind>)) : new Map();
  } catch {
    return new Map();
  }
};

interface Menu {
  sx: number;
  sy: number;
  wx: number;
  wy: number;
}

const Canvas = () => {
  const { activeId, activeState, saveActiveState, tabs, activeTabId, moveTileToTab } = useWorkspace();
  const {
    view,
    tiles,
    panTo,
    bgRef,
    frames,
    endPan,
    marquee,
    selected,
    toggleSelect,
    clearSelection,
    addNote,
    noteRenderDefault,
    addTile,
    addCode,
    addRunView,
    patchTile,
    addFrame,
    gridRef,
    duplicateTile,
    focusTile,
    focusFrame,
    onWheel,
    moveTile,
    snapTile,
    linkNoteTo,
    unlinkNoteFrom,
    linkTermTo,
    unlinkTermFrom,
    dragFrame,
    closeTile,
    reopenTile,
    snapFrame,
    activeTile,
    resetZoom,
    resizeTile,
    removeFrame,
    renameFrame,
    resizeFrame,
    recolorFrame,
    removeFrameWithTiles,
    fitFrame,
    frameSelection,
    activateTile,
    setTileCwd,
    setTileOscTitle,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  } = useCanvas({ seed: activeState, wsId: activeId, onPersist: saveActiveState });

  const [menu, setMenu] = React.useState<Menu | null>(null);
  const [alerts, setAlerts] = React.useState<Map<string, NotifyKind>>(loadAlerts);
  const [agents, setAgents] = React.useState<Map<string, 'idle' | 'busy'>>(new Map());
  const [noteEditors, setNoteEditors] = React.useState<Record<string, EditorView>>({});
  const [size, setSize] = React.useState({ w: window.innerWidth, h: window.innerHeight });
  const [fsId, setFsId] = React.useState<string | null>(null);
  const [fsExit, setFsExit] = React.useState(false);
  const [navOpen, setNavOpen] = React.useState(() => localStorage.getItem('panorama:navOpen') === '1');
  const [paletteOpen, setPaletteOpen] = React.useState(false);
  const paletteRef = React.useRef(paletteOpen);
  paletteRef.current = paletteOpen;
  React.useEffect(() => {
    localStorage.setItem('panorama:navOpen', navOpen ? '1' : '0');
  }, [navOpen]);

  React.useEffect(() => {
    if (pendingFocusTileId) {
      const id = pendingFocusTileId;
      const tile = tiles.find((t) => t.id === id);
      if (tile) {
        pendingFocusTileId = null;
        setTimeout(() => {
          activateTile(id);
          focusTile(id, false);
        }, 150);
      }
    }
  }, [tiles, activeTabId, focusTile, activateTile]);

  const [diff, setDiff] = React.useState<{ root: string; file: string } | null>(null);
  const [diffFiles, setDiffFiles] = React.useState<string[]>([]);
  const [diffExit, setDiffExit] = React.useState(false);
  const diffTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const fsIdRef = React.useRef<string | null>(fsId);
  fsIdRef.current = fsId;
  const activeTileRef = React.useRef<string | null>(activeTile);
  activeTileRef.current = activeTile;
  const fsTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const activated = React.useRef<Set<string>>(new Set());

  const [linkDrag, setLinkDrag] = React.useState<{ srcId: string; x: number; y: number; over: string | null } | null>(null);
  const viewR = React.useRef(view);
  viewR.current = view;
  const tilesR = React.useRef(tiles);
  tilesR.current = tiles;
  const dragId = linkDrag?.srcId ?? null;

  const startLinkDrag = React.useCallback((srcId: string, e: React.PointerEvent) => {
    e.stopPropagation();
    e.preventDefault();
    setLinkDrag({ srcId, x: e.clientX, y: e.clientY, over: null });
  }, []);

  React.useEffect(() => {
    if (!dragId) return;
    const move = (e: PointerEvent) => {
      const v = viewR.current;
      const wx = (e.clientX - v.x) / v.k;
      const wy = (e.clientY - v.y) / v.k;
      const term = tilesR.current.find(
        (t) => t.type === 'term' && t.id !== dragId && !t.runCwd && wx >= t.x && wx <= t.x + t.width && wy >= t.y && wy <= t.y + t.height
      );
      setLinkDrag((d) => (d ? { ...d, x: e.clientX, y: e.clientY, over: term?.id ?? null } : d));
    };
    const up = () =>
      setLinkDrag((d) => {
        if (d?.over) {
          const src = tilesR.current.find((t) => t.id === d.srcId);
          if (src?.type === 'term') linkTermTo(d.srcId, d.over);
          else linkNoteTo(d.srcId, d.over);
        }
        return null;
      });
    window.addEventListener('pointermove', move);
    window.addEventListener('pointerup', up);
    return () => {
      window.removeEventListener('pointermove', move);
      window.removeEventListener('pointerup', up);
    };
  }, [dragId, linkNoteTo, linkTermTo]);

  React.useEffect(() => {
    const onResize = () => setSize({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  React.useEffect(() => () => clearTimeout(diffTimer.current), []);

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
      if (cmd === 'tile.reopen') {
        reopenTile();
        return true;
      }
      if (cmd === 'tile.focus') {
        const id = activeTileRef.current;
        if (!id) return false;
        focusTile(id, true);
        return true;
      }
      if (cmd === 'view.resetZoom') {
        resetZoom();
        return true;
      }
      if (cmd === 'view.navigator') {
        setNavOpen((v) => !v);
        return true;
      }
      if (cmd === 'view.palette') {
        setPaletteOpen((v) => !v);
        return true;
      }
      return false;
    };
    let armedAt = 0;
    let cleanHold = false;
    const onKey = (e: KeyboardEvent) => {
      if (isCapturing()) return;
      if (e.key === 'Shift') {
        if (e.repeat || e.ctrlKey || e.altKey || e.metaKey) return;
        if (getBinding('view.palette') !== 'shift shift') return;
        if (performance.now() - armedAt < 350) {
          armedAt = 0;
          cleanHold = false;
          setPaletteOpen((v) => !v);
          e.preventDefault();
          e.stopPropagation();
          return;
        }
        cleanHold = true;
        return;
      }
      armedAt = 0;
      cleanHold = false;
      const cmd = matchCommand(e);
      if (!cmd) return;
      if (paletteRef.current && cmd !== 'view.palette') return;
      if (!run(cmd)) return;
      e.preventDefault();
      e.stopPropagation();
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key !== 'Shift' || !cleanHold) return;
      cleanHold = false;
      armedAt = performance.now();
    };
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('keyup', onKeyUp, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('keyup', onKeyUp, true);
    };
  }, [toggleFs, addTile, addNote, closeTile, reopenTile, resetZoom, focusTile]);

  React.useEffect(() => {
    if (!fsId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') exitFs();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [fsId, exitFs]);

  React.useEffect(() => {
    if (!selected.size) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') clearSelection();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [selected, clearSelection]);

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

  const preActive = React.useRef<string | null>(null);
  const lastDownAt = React.useRef(0);

  const onBgPointerDownCapture = (e: React.PointerEvent) => {
    const now = e.timeStamp;
    if (now - lastDownAt.current > DBLCLICK_MS) preActive.current = activeTile;
    lastDownAt.current = now;
  };

  const onBgDoubleClick = (e: React.MouseEvent) => {
    const el = (e.target as Element).closest('[data-tile]');
    const id = el?.getAttribute('data-tile');
    if (!id || id === preActive.current) return;
    focusTile(id, true);
  };

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
  const setNoteTitle = (id: string, title: string) => {
    const t = tiles.find((x) => x.id === id);
    if (!t) return;
    const next = applyFrontTitle(t.content || '', title);
    patchTile(id, { content: next });
    if (activeId) void writeNote(activeId, id, next).catch(() => {});
  };
  const setTileTitle = (id: string, title: string) => patchTile(id, { userTitle: title || undefined });
  const togglePin = (id: string) => patchTile(id, { pinned: !tiles.find((t) => t.id === id)?.pinned });
  const toggleNoteRaw = (id: string) => {
    const next = !tiles.find((t) => t.id === id)?.renderOnly;
    noteRenderDefault.current = next;
    patchTile(id, { renderOnly: next });
  };

  const copyTilePath = (id: string) => {
    const cwd = tiles.find((t) => t.id === id)?.cwd;
    if (cwd) writeClipboard(cwd);
  };

  const revealTilePath = (id: string) => {
    const cwd = tiles.find((t) => t.id === id)?.cwd;
    if (cwd) revealPath(cwd);
  };

  const copyNote = (id: string) => {
    const editor = noteEditors[id];
    if (editor) writeClipboard(editor.state.doc.toString());
  };

  const copyNoteSelection = (id: string) => {
    const editor = noteEditors[id];
    if (!editor) return;
    const { from, to } = editor.state.selection.main;
    writeClipboard(from === to ? editor.state.doc.toString() : editor.state.sliceDoc(from, to));
  };

  const pasteNote = async (id: string) => {
    const editor = noteEditors[id];
    if (!editor) return;
    const text = await readClipboard();
    if (!text) return;
    const { from, to } = editor.state.selection.main;
    editor.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length } });
    editor.focus();
  };

  const registerEditor = React.useCallback((id: string, editor: EditorView | null) => {
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

  const addAlert = React.useCallback((id: string, kind: NotifyKind) => {
    setAlerts((prev) => {
      if (prev.get(id) === kind) return prev;
      return new Map(prev).set(id, kind);
    });
  }, []);

  const setAgentState = React.useCallback((id: string, live: boolean, busy: boolean) => {
    setAgents((prev) => {
      const next = live ? (busy ? 'busy' : 'idle') : undefined;
      if (prev.get(id) === next) return prev;
      const m = new Map(prev);
      if (next) m.set(id, next);
      else m.delete(id);
      return m;
    });
  }, []);

  const clearAlert = React.useCallback((id: string) => {
    setAlerts((prev) => {
      if (!prev.has(id)) return prev;
      const next = new Map(prev);
      next.delete(id);
      return next;
    });
  }, []);

  React.useEffect(() => {
    if (activeTile) clearAlert(activeTile);
  }, [activeTile, clearAlert]);

  const activateAndClear = React.useCallback(
    (id: string) => {
      clearAlert(id);
      activateTile(id);
    },
    [clearAlert, activateTile]
  );

  const openNotified = React.useCallback(
    (id: string) => {
      activateAndClear(id);
      focusTile(id, true);
    },
    [activateAndClear, focusTile]
  );

  const navFocus = React.useCallback(
    (id: string, zoomToMax = false) => {
      activateAndClear(id);
      focusTile(id, zoomToMax);
    },
    [activateAndClear, focusTile]
  );

  const hideNav = () => setNavOpen(false);
  const showNav = () => setNavOpen(true);

  const closePalette = React.useCallback(() => setPaletteOpen(false), []);

  const paletteSelect = React.useCallback(
    (id: string) => {
      if (fsIdRef.current && fsIdRef.current !== id) exitFs();
      navFocus(id, true);
    },
    [exitFs, navFocus]
  );

  const openDiff = (root: string, file: string) => {
    clearTimeout(diffTimer.current);
    setDiffExit(false);
    setDiff({ root, file });
  };

  const closeDiff = () => {
    setDiffExit(true);
    diffTimer.current = setTimeout(() => {
      setDiff(null);
      setDiffExit(false);
    }, DIFF_ANIM);
  };

  const diffToCanvas = () => {
    if (!diff) return;
    addCode(diff.root, diff.file);
    closeDiff();
  };

  const diffAt = diff ? diffFiles.indexOf(diff.file) : -1;
  const stepFile = (step: number) => () => {
    if (!diff) return;
    setDiff({ root: diff.root, file: diffFiles[diffAt + step] });
  };

  const moveTileToTabWrapper = React.useCallback(
    (tileId: string, targetTabId: string) => {
      pendingFocusTileId = tileId;
      void moveTileToTab(tileId, { tiles, frames, view }, targetTabId);
    },
    [moveTileToTab, tiles, frames, view]
  );

  useNotifyBridge({ tiles, activeTile, onOpen: openNotified, onAlert: addAlert, onClear: clearAlert });

  React.useEffect(() => {
    void invoke('set_pending_count', { count: alerts.size }).catch(() => {});
    localStorage.setItem(ALERTS_KEY, JSON.stringify(Object.fromEntries(alerts)));
  }, [alerts]);

  return (
    <div className={fsId ? `${styles.root} ${styles.rootFs}` : styles.root}>
      <div
        ref={bgRef}
        className={styles.bg}
        onWheel={onWheel}
        onPointerUp={endPan}
        onContextMenu={openMenu}
        onDoubleClick={onBgDoubleClick}
        onPointerDown={onBgPointerDown}
        onPointerMove={onBgPointerMove}
        onPointerCancel={endPan}
        onPointerDownCapture={onBgPointerDownCapture}
      >
        <canvas ref={gridRef} className={styles.grid} />
        {!fsId && (
          <svg className={styles.links} width={size.w} height={size.h}>
            {tiles.map((t) => {
              if ((t.type !== 'note' && t.type !== 'term') || !t.linkedTo?.length) return null;
              const srcRect = { x: t.x * view.k + view.x, y: t.y * view.k + view.y, width: t.width * view.k, height: t.height * view.k };
              return t.linkedTo.map((termId) => {
                const term = tiles.find((x) => x.id === termId);
                if (!term) return null;
                const termRect = { x: term.x * view.k + view.x, y: term.y * view.k + view.y, width: term.width * view.k, height: term.height * view.k };
                const d = flowPath(srcRect, termRect);
                const removeLink = (e: React.PointerEvent) => {
                  e.stopPropagation();
                  if (t.type === 'term') unlinkTermFrom(t.id, termId);
                  else unlinkNoteFrom(t.id, termId);
                };
                return (
                  <g key={`${t.id}-${termId}`} className={styles.linkGroup} onPointerDown={removeLink}>
                    <path d={d} className={styles.linkHit} />
                    <path d={d} className={styles.linkLine} />
                  </g>
                );
              });
            })}
            {linkDrag &&
              (() => {
                const src = tiles.find((x) => x.id === linkDrag.srcId);
                if (!src) return null;
                const srcRect = { x: src.x * view.k + view.x, y: src.y * view.k + view.y, width: src.width * view.k, height: src.height * view.k };
                const d = flowPath(srcRect, { x: linkDrag.x, y: linkDrag.y, width: 0, height: 0 });
                return <path d={d} className={styles.linkLine} />;
              })()}
            {linkDrag?.over &&
              (() => {
                const term = tiles.find((x) => x.id === linkDrag.over);
                if (!term) return null;
                return (
                  <rect
                    x={term.x * view.k + view.x}
                    y={term.y * view.k + view.y}
                    width={term.width * view.k}
                    height={term.height * view.k}
                    rx={10}
                    className={styles.linkHover}
                  />
                );
              })()}
          </svg>
        )}
        {!fsId &&
          frames.map((f) => (
            <Frame key={f.id} frame={f} view={view} onSnap={snapFrame} onResize={resizeFrame} />
          ))}
        {tiles.map((t) => {
          const vis = isVisible(t);
          if (vis && (t.width - TILE_GAP) * view.k >= MIN_LIVE_WIDTH) activated.current.add(t.id);
          const live = activated.current.has(t.id);
          const linkedIds =
            t.type === 'note'
              ? t.linkedTo ?? []
              : t.type === 'term' && !t.runCwd
                ? [...new Set([...(t.linkedTo ?? []), ...tiles.filter((x) => x.type === 'term' && (x.linkedTo ?? []).includes(t.id)).map((x) => x.id)])]
                : [];
          const linkActive = t.type === 'note' && linkedIds.some((id) => id === activeTile || selected.has(id));
          const cand = t.type === 'note' ? adjacentTerm(t, tiles) : null;
          const linkCand = cand && !linkedIds.includes(cand.id) ? cand : null;
          const linkedTerms = linkedIds
            .map((id) => tiles.find((x) => x.id === id))
            .filter((x): x is typeof tiles[number] => Boolean(x))
            .map((term) => ({ id: term.id, name: termName(term) }));
          return (
            <TileFrame
              key={t.id}
              tile={t}
              view={view}
              wsId={activeId}
              linkActive={linkActive}
              linkTarget={linkCand ? { id: linkCand.id, name: termName(linkCand) } : null}
              linkedTerms={linkedTerms}
              onLink={linkNoteTo}
              onUnlink={t.type === 'term' ? unlinkTermFrom : unlinkNoteFrom}
              onLinkDragStart={startLinkDrag}
              onMove={moveTile}
              onSnap={snapTile}
              onClose={closeTile}
              onResize={resizeTile}
              onActivate={activateAndClear}
              onFocusTile={focusTile}
              onToggleFullscreen={toggleFs}
              onCwd={setTileCwd}
              onAgentState={setAgentState}
              onOscTitle={setTileOscTitle}
              onNoteChange={setNoteContent}
              onNoteEditor={registerEditor}
              onNoteTitle={setNoteTitle}
              onCopyNote={copyNote}
              onCopyNoteSelection={copyNoteSelection}
              onPasteNote={pasteNote}
              onToggleRaw={toggleNoteRaw}
              onRename={setTileTitle}
              onCopyPath={copyTilePath}
              onReveal={revealTilePath}
              onDuplicate={duplicateTile}
              onTogglePin={togglePin}
              onToggleSelect={toggleSelect}
              onOpenRunOutput={addRunView}
              onMoveToTab={moveTileToTabWrapper}
              tabs={tabs}
              activeTabId={activeTabId}
              active={t.id === activeTile}
              selected={selected.has(t.id)}
              alert={alerts.get(t.id) ?? null}
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
              onFit={fitFrame}
              onDrag={dragFrame}
              onSnap={snapFrame}
              onRemove={removeFrame}
              onRename={renameFrame}
              onRecolor={recolorFrame}
              onRemoveWithTiles={removeFrameWithTiles}
            />
          ))}
        {marquee && (
          <div
            className={styles.marquee}
            style={{ left: marquee.x, top: marquee.y, width: marquee.width, height: marquee.height }}
          />
        )}
        <div ref={indicatorRef} className={styles.indicator}>
          100%
        </div>
        {!fsId && <Minimap view={view} tiles={tiles} viewportRef={bgRef} onPan={panTo} />}
      </div>
      {paletteOpen && <Palette tiles={tiles} onSelect={paletteSelect} onClose={closePalette} />}
      {!fsId && navOpen && (
        <Navigator
          tiles={tiles}
          frames={frames}
          alerts={alerts}
          agents={agents}
          activeTile={activeTile}
          onNewTile={addTile}
          onFocusTile={navFocus}
          onFocusFrame={focusFrame}
          onRenameTile={setTileTitle}
          onCloseTile={closeTile}
          activeDiff={diff?.file ?? null}
          onDiffFiles={setDiffFiles}
          onOpenDiff={openDiff}
          onClose={hideNav}
        />
      )}
      {!fsId && diff && (
        <DiffViewer
          root={diff.root}
          file={diff.file}
          exiting={diffExit}
          onClose={closeDiff}
          onPrevFile={diffAt > 0 ? stepFile(-1) : undefined}
          onNextFile={diffAt >= 0 && diffAt < diffFiles.length - 1 ? stepFile(1) : undefined}
          onAddToCanvas={diffToCanvas}
        />
      )}
      {!fsId && !navOpen && (
        <div className={styles.navRevealZone}>
          <button
            className={styles.navReveal}
            onClick={showNav}
            data-tooltip="Show menu"
            data-shortcut={formatCombo(getBinding('view.navigator'))}
            aria-label="Show menu"
          />
        </div>
      )}
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
            { label: 'New frame', icon: <SquareDashed size={15} strokeWidth={1.75} />, onSelect: newFrame },
            ...(selected.size
              ? [{ label: 'Frame selection', icon: <Group size={15} strokeWidth={1.75} />, onSelect: frameSelection }]
              : [])
          ]}
        />
      )}
    </div>
  );
};

export default Canvas;
