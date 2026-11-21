import React from 'react';
import type { EditorView } from '@codemirror/view';
import { Group, LayoutGrid, StickyNote, SquareDashed, SquareTerminal } from 'lucide-react';

import { getPref, setPref } from '~/usecase/util/prefs';
import { revealPath } from '~/adapter/shell/shell.client';
import { readClipboard, writeClipboard } from '~/adapter/clipboard/clipboard.client';
import Frame from '~/components/Canvas/Frame';
import Toasts from '~/components/commons/Toasts';
import FrameBar, { type FrameBoardProp } from '~/components/Canvas/FrameBar';
import Minimap from '~/components/Canvas/Minimap';
import CommandBar from '~/components/Canvas/CommandBar';
import SplitHandle from '~/components/Canvas/SplitHandle';
import TileFrame from '~/components/Canvas/TileFrame';
import Workbench, { type WorkbenchHandlers } from '~/components/Workbench';
import UnsavedDialog from '~/components/commons/UnsavedDialog';
import CanvasDock, { type CanvasDockHandlers } from '~/components/Canvas/CanvasDock';
import { useDockLayout } from '~/usecase/hooks/useDockLayout';
import { dockerAvailable } from '~/adapter/docker/docker.client';
import { visiblePanels } from '~/usecase/util/dockLayout';
import { PANEL_DESCRIPTORS } from '~/components/commons/Dock/panels';
import NoteToolbar from '~/components/Canvas/NoteToolbar';
import ContextMenu from '~/components/commons/ContextMenu';
import { useCanvas } from '~/usecase/hooks/useCanvas';
import { useRemoteTiles } from '~/usecase/hooks/useRemoteTiles';
import { useBackdrop } from '~/usecase/hooks/useBackdrop';
import { writeNote } from '~/adapter/notes/notes.client';
import { fileKey, splitKey } from '~/usecase/util/fileTree';
import { applyFrontTitle } from '~/usecase/util/noteMeta';
import { adjacentTerm, termName, flowPath } from '~/usecase/util/noteLink';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { TileHostProvider } from '~/usecase/context/TileHostContext';
import { workspaceService } from '~/usecase/service/workspace.service';
import { useNotifyBridge } from '~/components/commons/Notifications/bridge';
import { TILE_GAP, CULL_MARGIN, MIN_LIVE_WIDTH, TILE_WIDTH, TILE_HEIGHT, NOTE_WIDTH, NOTE_HEIGHT, TOOLBAR_HEIGHT } from '~/usecase/util/constants';
import { showToast } from '~/usecase/util/toasts';
import { buildProjectGroups } from '~/usecase/util/projectGroups';
import { spawnInGroup, fixGroupFrame } from '~/usecase/util/projectGroupActions';
import { syncProjectViewWithTiles, projectName, projectRootPath, normalizeProjectPath } from '~/usecase/util/projectLayout';
import { useProjectsPanelActions } from '~/usecase/hooks/useProjectsPanelActions';
import AddProjectModal from '~/components/ProjectView/AddProjectModal';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';
import { getAlerts, setAlert, clearAlert, subscribeAlerts } from '~/usecase/util/alerts';
import { setAgentStatus, getAgentStatuses, subscribeAgentStatuses } from '~/usecase/util/agentStatus';
import { isCapturing, getBinding, formatCombo, matchCommand, type CommandId } from '~/usecase/util/keybindings';
import { isDirty, requestSave, requestFind, dispatchSave, subscribeDirty, requestPreview } from '~/usecase/util/dirtyFiles';
import { type ShowWorkerRequest, onOrchBoardRequest, onOrchDiffRequest, onShowWorkerRequest, onNewBoardRequest, getProjectLastBoard, setProjectLastBoard } from '~/usecase/util/orchBoardRequest';
import { coordinatorActions, useOrchRuns } from '~/usecase/hooks/useOrchCoordinator';
import { tileBoardProject, projectBoards } from '~/usecase/util/boardProject';
import { normalizeRepo } from '~/usecase/util/orch';
import { gitRoot } from '~/adapter/git/git.client';
import { frameOf } from '~/usecase/util/frame';
import NewBoardDialog from '~/components/commons/NewBoardDialog';
import { onRefDiff } from '~/usecase/util/refDiff';
import { touchTile } from '~/usecase/util/tileRecency';
import { getCanvasSidebarMode, CANVAS_SIDEBAR_MODE_EVENT } from '~/usecase/util/canvasSidebars';
import type { CommandBarSources } from '~/usecase/hooks/useCommandBarItems';
import { tabKey, openTab, pinTab, type OpenRequest } from '~/usecase/util/editorTabs';
import type { FsBox } from '~/domain/interfaces/canvas.interface';
import type { EditorTab } from '~/domain/interfaces/editor.interface';
import type { PanelId } from '~/domain/interfaces/dock.interface';
import type { NotifyKind, NotifyTarget } from '~/domain/interfaces/notify.interface';

import styles from './styles.module.scss';

const FS_ANIM = 170;
const DIFF_ANIM = 130;

const FS_GAP = 8;
const FS_MIN = 320;
const SPLIT_KEY = 'panorama:fsSplit';

const savedSplit = (): number => {
  const raw = Number(getPref(SPLIT_KEY));
  return raw > 0 && raw < 1 ? raw : 0.5;
};

const DBLCLICK_MS = 400;

let pendingFocusTileId: string | null = null;

const jumpToWorkspaceTab = async (wsId: string, tabId: string, switchWorkspace: (id: string) => Promise<void>): Promise<void> => {
  await workspaceService.setActiveTab(wsId, tabId);
  await switchWorkspace(wsId);
};

interface Menu {
  sx: number;
  sy: number;
  wx: number;
  wy: number;
}

const Canvas = () => {
  const {
    tabs,
    activeId,
    switchTab,
    setTabMode,
    workspaces,
    activeState,
    activeTabId,
    moveTileToTab,
    switchWorkspace,
    saveActiveState
  } = useWorkspace();
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
    openRemoteTile,
    addCode,
    addEditor,
    addRunView,
    patchTile,
    addFrame,
    gridRef,
    duplicateTile,
    focusTile,
    centerTile,
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
    settleFrameDrag,
    activeTile,
    resetZoom,
    resizeTile,
    removeFrame,
    renameFrame,
    createFrame,
    patchFrame,
    placeTiles,
    resizeFrame,
    recolorFrame,
    removeFrameWithTiles,
    fitFrame,
    reorderFrame,
    reorderCanvas,
    reorderProjects,
    reorderFrameGroups,
    undoLayout,
    redoLayout,
    frameSelection,
    activateTile,
    setTileCwd,
    setTileOscTitle,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown,
    addOrchBoard
  } = useCanvas({ seed: activeState, wsId: activeId, tabId: activeTabId, onPersist: saveActiveState });

  const handleShowWorkerRef = React.useRef<(req: ShowWorkerRequest) => void>(() => {});

  React.useEffect(() => {
    const unsubBoard = onOrchBoardRequest(({ runId, nearTileId }) => addOrchBoard(runId, nearTileId));
    const unsubWorker = onShowWorkerRequest((req) => handleShowWorkerRef.current(req));
    const unsubNewBoard = onNewBoardRequest(({ repo, tileId }) => {
      const proj = tileBoardProject(framesR.current, tilesR.current, tileId);
      if (proj.kind === 'project') {
        setNewBoard({ repo: proj.repo, nearTileId: tileId, frameId: proj.frameId });
        return;
      }
      setNewBoard({ repo: projectRootPath(repo), nearTileId: tileId });
    });
    return () => { unsubBoard(); unsubWorker(); unsubNewBoard(); };
  }, [addOrchBoard]);

  const [menu, setMenu] = React.useState<Menu | null>(null);
  const alerts = React.useSyncExternalStore(subscribeAlerts, getAlerts);
  const agents = React.useSyncExternalStore(subscribeAgentStatuses, getAgentStatuses);
  const [noteEditors, setNoteEditors] = React.useState<Record<string, EditorView>>({});
  const [size, setSize] = React.useState({ w: window.innerWidth, h: window.innerHeight });
  const [fsId, setFsId] = React.useState<string | null>(null);
  const [fsExit, setFsExit] = React.useState(false);
  const [navOpen, setNavOpen] = React.useState(() => getPref('panorama:navOpen') === '1');
  const [dockRightOpen, setDockRightOpen] = React.useState(() => {
    const v = getPref('panorama:dockRightOpen');
    return v === null || v === '1';
  });
  const backdrop = useBackdrop();
  const [navWidth, setNavWidth] = React.useState(0);
  const [dockRightWidth, setDockRightWidth] = React.useState(0);
  const [sidebarMode, setSidebarMode] = React.useState(getCanvasSidebarMode);
  const [fsSplit, setFsSplit] = React.useState(savedSplit);
  const [commandBarOpen, setCommandBarOpen] = React.useState(false);
  const commandBarRef = React.useRef(commandBarOpen);
  commandBarRef.current = commandBarOpen;
  React.useEffect(() => {
    setPref('panorama:navOpen', navOpen ? '1' : '0');
  }, [navOpen]);
  React.useEffect(() => {
    setPref('panorama:dockRightOpen', dockRightOpen ? '1' : '0');
  }, [dockRightOpen]);
  React.useEffect(() => {
    setPref(SPLIT_KEY, String(fsSplit));
  }, [fsSplit]);
  React.useEffect(() => {
    const onMode = () => setSidebarMode(getCanvasSidebarMode());
    window.addEventListener(CANVAS_SIDEBAR_MODE_EVENT, onMode);
    return () => window.removeEventListener(CANVAS_SIDEBAR_MODE_EVENT, onMode);
  }, []);

  const { layout: canvasLayout, reset: resetCanvasLayout } = useDockLayout('canvas');
  const [hasDocker, setHasDocker] = React.useState(false);

  React.useEffect(() => {
    dockerAvailable()
      .then(setHasDocker)
      .catch(() => setHasDocker(false));
  }, []);

  const panelAvailable = React.useCallback((id: PanelId) => id !== 'docker' || hasDocker, [hasDocker]);
  const leftPanels = visiblePanels(canvasLayout, 'left', panelAvailable).map((id) => PANEL_DESCRIPTORS[id]);
  const rightPanels = visiblePanels(canvasLayout, 'right', panelAvailable).map((id) => PANEL_DESCRIPTORS[id]);
  const leftHasVisible = leftPanels.length > 0;
  const rightHasVisible = rightPanels.length > 0;

  React.useLayoutEffect(() => {
    const id = pendingFocusTileId;
    if (!id || !tiles.some((t) => t.id === id)) return;
    pendingFocusTileId = null;
    activateTile(id);
    if (!centerTile(id)) focusTile(id, false);
  }, [tiles, activeTabId, focusTile, centerTile, activateTile]);

  const [editorTabs, setEditorTabs] = React.useState<EditorTab[]>([]);
  const [activeEditorTab, setActiveEditorTab] = React.useState<string | null>(null);
  const [tileToClose, setTileToClose] = React.useState<{ id: string; path: string } | null>(null);
  const closeTileRef = React.useRef<(id: string) => void>(() => {});
  const [diffFiles, setDiffFiles] = React.useState<string[]>([]);
  const [refDiffFiles, setRefDiffFiles] = React.useState<string[]>([]);
  const [tabsExit, setTabsExit] = React.useState(false);
  const tabsTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const fsIdRef = React.useRef<string | null>(fsId);
  fsIdRef.current = fsId;
  const activeTileRef = React.useRef<string | null>(activeTile);
  activeTileRef.current = activeTile;
  const tabsOpenRef = React.useRef(false);
  tabsOpenRef.current = editorTabs.length > 0;
  const fsTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const activated = React.useRef<Set<string>>(new Set());

  const openPhoneTile = (id: string, cwd: string, group?: { id: string; title: string }) => {
    activated.current.add(id);
    openRemoteTile(id, cwd, group);
  };
  useRemoteTiles(openPhoneTile);

  const [linkDrag, setLinkDrag] = React.useState<{ srcId: string; x: number; y: number; over: string | null } | null>(null);
  const viewR = React.useRef(view);
  viewR.current = view;
  const tilesR = React.useRef(tiles);
  tilesR.current = tiles;
  const framesR = React.useRef(frames);
  framesR.current = frames;
  const dragId = linkDrag?.srcId ?? null;
  const allRuns = useOrchRuns();
  const [newBoard, setNewBoard] = React.useState<{ repo: string; nearTileId?: string; frameId?: string } | null>(null);

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
        (t) =>
          t.type === 'term' &&
          t.id !== dragId &&
          !t.runCwd &&
          wx >= t.x &&
          wx <= t.x + t.width &&
          wy >= t.y &&
          wy <= t.y + t.height
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

  React.useEffect(() => () => clearTimeout(tabsTimer.current), []);

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

  const runCommand = (cmd: CommandId): boolean => {
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
      if (!id || tabsOpenRef.current) return false;
      closeTileRef.current(id);
      return true;
    }
    if (cmd === 'tile.reopen') {
      reopenTile();
      return true;
    }
    if (cmd === 'canvas.reorder') {
      if (fsIdRef.current) return false;
      reorderCanvas();
      return true;
    }
    if (cmd === 'canvas.undo') return !fsIdRef.current && undoLayout();
    if (cmd === 'canvas.redo' || cmd === 'canvas.redoAlt') return !fsIdRef.current && redoLayout();
    if (cmd === 'tile.focus') {
      const id = activeTileRef.current;
      if (!id) return false;
      focusTile(id, true);
      return true;
    }
    if (cmd === 'editor.save') return dispatchSave().handled;
    if (cmd === 'editor.find') return requestFind();
    if (cmd === 'editor.preview') return requestPreview();
    if (cmd === 'view.resetZoom') {
      resetZoom();
      return true;
    }
    if (cmd === 'view.navigator') {
      if (leftHasVisible) setNavOpen((v) => !v);
      return true;
    }
    if (cmd === 'view.inspector') {
      if (rightHasVisible) setDockRightOpen((v) => !v);
      return true;
    }
    if (cmd === 'view.resetPanels') {
      resetCanvasLayout();
      return true;
    }
    if (cmd === 'view.palette') {
      setCommandBarOpen((v) => !v);
      return true;
    }
    if (cmd === 'view.toggleMode') {
      if (!activeTabId) return false;
      void setTabMode(activeTabId, 'project');
      return true;
    }
    if (cmd === 'orch.toggleMode') {
      if (activeTile) coordinatorActions.toggle(activeTile);
      return true;
    }
    if (cmd === 'orch.boards') {
      const tileId = activeTileRef.current;
      if (!tileId) return false;
      const proj = tileBoardProject(framesR.current, tilesR.current, tileId);
      if (proj.kind === 'loose') {
        boardCreateProject(tileId);
      } else if (proj.kind === 'noFolder') {
        projectActions.onSetFolder(proj.frameId);
      } else {
        const lastBoard = getProjectLastBoard(normalizeRepo(proj.repo));
        if (lastBoard) {
          addOrchBoard(lastBoard, tileId, proj.repo, proj.frameId);
        } else {
          setNewBoard({ repo: proj.repo, nearTileId: tileId, frameId: proj.frameId });
        }
      }
      return true;
    }
    return false;
  };
  const runCommandRef = React.useRef(runCommand);
  runCommandRef.current = runCommand;

  React.useEffect(() => {
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
          setCommandBarOpen((v) => !v);
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
      if (commandBarRef.current && cmd !== 'view.palette') return;
      if (!runCommandRef.current(cmd)) return;
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
  }, []);

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

  const navInset = navOpen && leftHasVisible ? navWidth + 8 : 0;
  const rightInset = dockRightOpen && rightHasVisible ? dockRightWidth + 8 : 0;
  const areaW = size.w - 18 - navInset - rightInset;
  const dockedLeft = sidebarMode === 'docked' ? navInset : 0;
  const dockedRight = sidebarMode === 'docked' ? rightInset : 0;
  const prevDocked = React.useRef({ left: dockedLeft, right: dockedRight });
  React.useLayoutEffect(() => {
    const dLeft = dockedLeft - prevDocked.current.left;
    const dRight = dockedRight - prevDocked.current.right;
    prevDocked.current = { left: dockedLeft, right: dockedRight };
    // the viewport ResizeObserver re-centers by half the width change; this cancels it so tiles stay put on screen
    if (dLeft !== 0 || dRight !== 0) panTo(view.x + (dRight - dLeft) / 2, view.y);
  }, [dockedLeft, dockedRight]);
  const bgStyle =
    sidebarMode === 'docked'
      ? ({ left: `${8 + dockedLeft}px`, right: `${8 + dockedRight}px`, '--dock-right-inset': '0px' } as React.CSSProperties)
      : undefined;
  const splitting = fsId !== null && activeEditorTab !== null;
  const fsWidth = splitting ? Math.min(Math.max(Math.round(areaW * fsSplit), FS_MIN), areaW - FS_MIN - FS_GAP) : areaW;
  const fsBox: FsBox = { x: navInset - dockedLeft, y: 0, width: fsWidth, height: size.h - 72 };
  const benchOffset = { ['--bench-offset' as string]: splitting ? `${fsWidth + FS_GAP}px` : '0px' };

  const moveSplit = (clientX: number) => {
    const ratio = (clientX - 9 - navInset - FS_GAP / 2) / areaW;
    setFsSplit(Math.min(0.9, Math.max(0.1, ratio)));
  };

  const inset = TILE_GAP / 2;
  const isVisible = (t: (typeof tiles)[number]): boolean => {
    const left = (t.x + inset) * view.k + view.x;
    const top = (t.y + inset) * view.k + view.y;
    const w = (t.width - TILE_GAP) * view.k;
    const h = (t.height - TILE_GAP) * view.k;
    return left < size.w + CULL_MARGIN && left + w > -CULL_MARGIN && top < size.h + CULL_MARGIN && top + h > -CULL_MARGIN;
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
    if (!menu) return;
    const frame = frameOf(framesR.current, { x: menu.wx, y: menu.wy, width: 0, height: 0 });
    addTile({ x: menu.wx, y: menu.wy }, frame?.path);
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

  const patchOrchTile = React.useCallback((id: string, patch: { runId?: string; boardTitle?: string }) => patchTile(id, patch), [patchTile]);

  const activeNote = tiles.find((t) => t.id === activeTile && t.type === 'note') ?? null;
  const activeNoteEditor = activeNote ? noteEditors[activeNote.id] : null;

  const newFrame = () => {
    if (menu) addFrame(menu.wx, menu.wy);
  };

  const alertKinds = React.useMemo(
    () => new Map([...alerts].map(([id, alert]): [string, NotifyKind] => [id, alert.kind])),
    [alerts]
  );

  const addAlert = React.useCallback((id: string, kind: NotifyKind, tabId: string | null) => {
    setAlert(id, kind, tabId ?? '');
  }, []);

  const setAgentState = React.useCallback((id: string, live: boolean, busy: boolean) => {
    setAgentStatus(id, live ? (busy ? 'busy' : 'idle') : null);
  }, []);

  React.useEffect(() => {
    if (!activeTile) return;
    clearAlert(activeTile);
    touchTile(activeTile);
  }, [activeTile]);

  const activateAndClear = React.useCallback(
    (id: string) => {
      clearAlert(id);
      activateTile(id);
    },
    [activateTile]
  );

  const openNotified = React.useCallback(
    ({ tileId, wsId, tabId }: NotifyTarget) => {
      if (tiles.some((t) => t.id === tileId)) {
        activateAndClear(tileId);
        focusTile(tileId, true);
        return;
      }
      if (!tabId) return;
      if (wsId && wsId !== activeId) {
        pendingFocusTileId = tileId;
        void jumpToWorkspaceTab(wsId, tabId, switchWorkspace);
        return;
      }
      if (!tabs.some((t) => t.id === tabId)) return;
      pendingFocusTileId = tileId;
      void switchTab(tabId);
    },
    [tiles, tabs, activeId, activateAndClear, focusTile, switchTab, switchWorkspace]
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
  const hideDockRight = () => setDockRightOpen(false);
  const showDockRight = () => setDockRightOpen(true);

  const closeCommandBar = React.useCallback(() => setCommandBarOpen(false), []);

  const focusLeavingFs = React.useCallback(
    (id: string, zoomToMax = false) => {
      if (fsIdRef.current && fsIdRef.current !== id) exitFs();
      navFocus(id, zoomToMax);
    },
    [exitFs, navFocus]
  );

  const commandBarFrame = (id: string) => {
    if (fsIdRef.current) exitFs();
    focusFrame(id);
  };

  const commandBarTab = (wsId: string, tabId: string) => {
    if (wsId === activeId) void switchTab(tabId);
    else void jumpToWorkspaceTab(wsId, tabId, switchWorkspace);
  };

  const commandBarSources: CommandBarSources = {
    tiles,
    frames,
    activeId,
    workspaces,
    activeTabId,
    alerts: alertKinds,
    actions: {
      openTab: commandBarTab,
      openTile: openNotified,
      focusFrame: commandBarFrame,
      focusTile: (id) => focusLeavingFs(id, true),
      runCommand: (id) => void runCommandRef.current(id),
      openWorkspace: (wsId) => void switchWorkspace(wsId)
    }
  };

  const openEditorTab = (request: OpenRequest) => {
    clearTimeout(tabsTimer.current);
    setTabsExit(false);
    setEditorTabs((prev) => openTab(prev, request));
    setActiveEditorTab(tabKey(request.kind, request.path));
  };

  const openEditorFile = (root: string, path: string, preview?: boolean) => openEditorTab({ kind: 'file', root, path, preview });
  const openDiff = (root: string, path: string, commit?: string) => openEditorTab({ kind: 'diff', root, path, commit });

  React.useEffect(
    () =>
      onRefDiff(({ root, base, path, files }) => {
        setRefDiffFiles(files.map((file) => fileKey(root, file)));
        openEditorTab({ kind: 'diff', root, path, base });
      }),
    []
  );

  React.useEffect(
    () => onOrchDiffRequest(({ root, file, commit, base }) => openEditorTab({ kind: 'diff', root, path: file, commit, base })),
    []
  );

  const pinEditorTab = (key: string) => setEditorTabs((prev) => pinTab(prev, key));

  React.useEffect(
    () =>
      subscribeDirty(() =>
        setEditorTabs((prev) =>
          prev.some((t) => t.preview && isDirty(t.path))
            ? prev.map((t) => (t.preview && isDirty(t.path) ? { ...t, preview: undefined } : t))
            : prev
        )
      ),
    []
  );

  const closeEditorTab = (key: string) => {
    setEditorTabs((prev) => {
      const next = prev.filter((t) => t.key !== key);
      setActiveEditorTab((cur) => (cur === key ? (next[next.length - 1]?.key ?? null) : cur));
      return next;
    });
  };

  const closeEditorTabs = () => {
    setTabsExit(true);
    tabsTimer.current = setTimeout(() => {
      setEditorTabs([]);
      setActiveEditorTab(null);
      setTabsExit(false);
    }, DIFF_ANIM);
  };

  const handleShowWorker = ({ tileId, cwd, group }: ShowWorkerRequest) => {
    if (editorTabs.length > 0) closeEditorTabs();
    if (tiles.some((t) => t.id === tileId)) {
      activateAndClear(tileId);
      focusTile(tileId, true);
      return;
    }
    openRemoteTile(tileId, cwd, group);
  };
  handleShowWorkerRef.current = handleShowWorker;

  const tabToCanvas = (tab: EditorTab) => {
    if (tab.kind === 'diff') addCode(tab.root, tab.path);
    else addEditor(tab.root, tab.path);
    closeEditorTab(tab.key);
  };

  const stepDiffFile = (step: number) => {
    const current = editorTabs.find((t) => t.key === activeEditorTab);
    if (current?.kind !== 'diff') return;
    const files = current.base ? refDiffFiles : diffFiles;
    const at = files.indexOf(fileKey(current.root, current.path));
    if (at < 0) return;
    const next = files[at + step];
    if (next) {
      const { repo, path } = splitKey(next);
      openEditorTab({ kind: 'diff', root: repo, path, base: current.base, commit: current.commit });
    }
  };

  const requestCloseTile = (id: string) => {
    const tile = tiles.find((t) => t.id === id);
    if (tile?.type === 'editor' && tile.filePath && isDirty(tile.filePath)) {
      setTileToClose({ id, path: tile.filePath });
      return;
    }
    closeTile(id);
  };
  closeTileRef.current = requestCloseTile;

  const discardTile = () => {
    if (tileToClose) closeTile(tileToClose.id);
    setTileToClose(null);
  };

  const saveAndCloseTile = async () => {
    if (!tileToClose) return;
    await requestSave(tileToClose.path);
    closeTile(tileToClose.id);
    setTileToClose(null);
  };

  const onSaveTile = () => void saveAndCloseTile();
  const cancelCloseTile = () => setTileToClose(null);

  const editTabFile = (tab: EditorTab) => {
    const sep = tab.root.includes('/') ? '/' : '\\';
    openEditorFile(tab.root, tab.root + sep + tab.path.replace(/\//g, sep));
  };

  const workbenchHandlers: WorkbenchHandlers = {
    onSelect: setActiveEditorTab,
    onCloseTab: closeEditorTab,
    onAddToCanvas: tabToCanvas,
    onEditFile: editTabFile,
    onStepFile: stepDiffFile,
    onPinTab: pinEditorTab,
    onClose: closeEditorTabs
  };

  const activeDiffTab = editorTabs.find((t) => t.key === activeEditorTab && t.kind === 'diff');
  const activeDiffFile = activeDiffTab ? fileKey(activeDiffTab.root, activeDiffTab.path) : null;

  const moveTileToTabWrapper = React.useCallback(
    (tileId: string, targetTabId: string) => {
      pendingFocusTileId = tileId;
      void moveTileToTab(tileId, { tiles, frames, view }, targetTabId);
    },
    [moveTileToTab, tiles, frames, view]
  );

  const baseDockHandlers = {
    onDiffFiles: setDiffFiles,
    onOpenDiff: openDiff,
    onOpenFile: openEditorFile
  };

  const leftDockHandlers: CanvasDockHandlers = { ...baseDockHandlers, onWidth: setNavWidth, onClose: hideNav };
  const rightDockHandlers: CanvasDockHandlers = { ...baseDockHandlers, onWidth: setDockRightWidth, onClose: hideDockRight };

  const folderPickerResolveRef = React.useRef<((p: string | null) => void) | null>(null);
  const [addProjectOpen, setAddProjectOpen] = React.useState(false);

  const canvasProjects = React.useMemo(
    () => syncProjectViewWithTiles(activeState?.projectView, tiles).projects,
    [activeState?.projectView, tiles]
  );

  const canvasGroups = React.useMemo(
    () => buildProjectGroups({ frames, tiles, projects: canvasProjects }),
    [frames, tiles, canvasProjects]
  );

  const canvasGroupOps = React.useMemo(
    () => ({ tiles, frames, createFrame, patchFrame, placeTiles }),
    [tiles, frames, createFrame, patchFrame, placeTiles]
  );

  const getCanvasCenter = React.useCallback(() => {
    const cx = (window.innerWidth / 2 - view.x) / view.k;
    const cy = ((window.innerHeight - TOOLBAR_HEIGHT) / 2 - view.y) / view.k;
    return { x: cx, y: cy };
  }, [view]);

  const openFolderPicker = React.useCallback((): Promise<string | null> => {
    return new Promise((resolve) => {
      folderPickerResolveRef.current = resolve;
      setAddProjectOpen(true);
    });
  }, []);

  const handleFolderPickerSelect = (path: string) => {
    setAddProjectOpen(false);
    const resolve = folderPickerResolveRef.current;
    folderPickerResolveRef.current = null;
    resolve?.(path);
  };

  const handleFolderPickerClose = () => {
    setAddProjectOpen(false);
    const resolve = folderPickerResolveRef.current;
    folderPickerResolveRef.current = null;
    resolve?.(null);
  };

  const canvasOnAddProject = React.useCallback(() => {
    void openFolderPicker().then((path) => {
      if (!path) return;
      const norm = normalizeProjectPath(path);
      const near = getCanvasCenter();
      const emptyGroup: ProjectGroup = {
        key: `path:${norm}`,
        kind: 'path',
        title: projectName(norm),
        path: norm,
        frameId: null,
        tiles: [],
        outside: [],
        drifted: []
      };
      fixGroupFrame({ ops: canvasGroupOps, group: emptyGroup, near });
    });
  }, [openFolderPicker, getCanvasCenter, canvasGroupOps]);

  const canvasSpawnTerminal = React.useCallback((group: ProjectGroup | null) => {
    if (group?.kind === 'frame' && group.frameId) {
      const result = spawnInGroup({ ops: canvasGroupOps, group, width: TILE_WIDTH, height: TILE_HEIGHT });
      if (result) {
        addTile({ x: result.x, y: result.y }, group.path ?? undefined);
        if (!result.fitted) showToast({ kind: 'info', message: 'No room in frame' });
      } else {
        addTile(undefined, group.path ?? undefined);
      }
    } else {
      addTile(undefined, group?.path ?? undefined);
    }
  }, [canvasGroupOps, addTile]);

  const canvasSpawnNote = React.useCallback((group: ProjectGroup) => {
    if (group.kind === 'frame' && group.frameId) {
      const result = spawnInGroup({ ops: canvasGroupOps, group, width: NOTE_WIDTH, height: NOTE_HEIGHT });
      if (result) {
        addNote({ x: result.x + NOTE_WIDTH / 2, y: result.y + NOTE_HEIGHT / 2 });
      } else {
        addNote();
      }
    } else {
      addNote();
    }
  }, [canvasGroupOps, addNote]);

  const canvasSelectGroupOp = React.useCallback((group: ProjectGroup) => {
    if (group.kind === 'frame' && group.frameId) {
      focusFrame(group.frameId);
    } else {
      const first = group.tiles[0] ?? group.outside[0];
      if (first) focusTile(first.id);
    }
  }, [focusFrame, focusTile]);

  const canvasGroupOpsRef = React.useRef(canvasGroupOps);
  canvasGroupOpsRef.current = canvasGroupOps;
  const getCanvasCenterRef = React.useRef(getCanvasCenter);
  getCanvasCenterRef.current = getCanvasCenter;

  const boardCreateProject = React.useCallback((tileId: string) => {
    const tile = tilesR.current.find((t) => t.id === tileId);
    if (!tile?.cwd) return;
    const cwd = tile.cwd;
    void gitRoot(cwd)
      .catch(() => normalizeProjectPath(cwd))
      .then((repo) => {
        const near = getCanvasCenterRef.current();
        const group: ProjectGroup = {
          key: `path:${repo}`,
          kind: 'path',
          title: projectName(repo),
          path: repo,
          frameId: null,
          tiles: [tile],
          outside: [],
          drifted: []
        };
        const frameId = fixGroupFrame({ ops: canvasGroupOpsRef.current, group, near });
        setNewBoard({ repo, nearTileId: tileId, frameId });
      });
  }, []);

  const boardOpenBoard = React.useCallback((runId: string, nearTileId?: string, repo?: string, frameId?: string) => {
    addOrchBoard(runId, nearTileId, repo, frameId);
  }, [addOrchBoard]);

  const boardNewBoard = React.useCallback((repo: string, nearTileId?: string, frameId?: string) => {
    setNewBoard({ repo, nearTileId, frameId });
  }, []);

  const projectActions = useProjectsPanelActions({
    ops: canvasGroupOps,
    removeFrameOp: removeFrame,
    renameFrameOp: renameFrame,
    activateTileOp: (tile) => activateTile(tile.id),
    closeTileOp: requestCloseTile,
    renameTileOp: setTileTitle,
    spawnTerminal: canvasSpawnTerminal,
    spawnNote: canvasSpawnNote,
    selectGroupOp: canvasSelectGroupOp,
    onAddProject: canvasOnAddProject,
    openFolderPicker,
    getViewCenter: getCanvasCenter,
    reorderProjectOp: reorderProjects,
    reorderFrameOp: reorderFrameGroups
  });

  const boards = React.useMemo(() => ({
    allRuns,
    projectOf: (tileId: string) => tileBoardProject(framesR.current, tilesR.current, tileId),
    openBoard: boardOpenBoard,
    newBoard: boardNewBoard,
    createProject: boardCreateProject,
    setFolder: projectActions.onSetFolder
  }), [allRuns, boardOpenBoard, boardNewBoard, boardCreateProject, projectActions.onSetFolder]);

  const handleNewBoardCreated = React.useCallback((runId: string) => {
    if (!newBoard) return;
    const { repo, nearTileId, frameId } = newBoard;
    setNewBoard(null);
    setProjectLastBoard(repo, runId);
    const orchTile = nearTileId ? tilesR.current.find((t) => t.id === nearTileId && t.type === 'orch' && !t.runId) : null;
    if (orchTile) {
      patchTile(orchTile.id, { runId });
    } else {
      addOrchBoard(runId, nearTileId, repo, frameId);
    }
  }, [newBoard, addOrchBoard, patchTile]);

  const boardPropsForFrame = React.useCallback((frame: { id: string; path?: string }): FrameBoardProp => {
    const frameRuns = frame.path ? projectBoards(allRuns, frame.path) : [];
    return {
      runs: frameRuns,
      onOpen: (runId, frameId) => addOrchBoard(runId, undefined, frame.path, frameId),
      onNew: () => setNewBoard({ repo: frame.path ?? '', frameId: frame.id }),
      onSetFolder: () => projectActions.onSetFolder(frame.id)
    };
  }, [allRuns, addOrchBoard, projectActions.onSetFolder]);

  const projectActionsWithBoards = React.useMemo(() => ({
    ...projectActions,
    allRuns,
    onOpenBoard: (runId: string, group: ProjectGroup) => addOrchBoard(runId, group.tiles[0]?.id, group.path ?? undefined, group.frameId ?? undefined),
    onNewBoard: (group: ProjectGroup) => {
      if (group.path) setNewBoard({ repo: group.path, frameId: group.frameId ?? undefined });
    }
  }), [projectActions, allRuns, addOrchBoard]);

  const activeGroupKey = React.useMemo(() => {
    if (!activeTile) return null;
    for (const group of canvasGroups) {
      if (group.tiles.some((t) => t.id === activeTile) || group.outside.some((t) => t.id === activeTile)) {
        return group.key;
      }
    }
    return null;
  }, [canvasGroups, activeTile]);

  useNotifyBridge({
    tiles,
    wsId: activeId,
    tabId: activeTabId,
    activeTile,
    onOpen: openNotified,
    onAlert: addAlert,
    onClear: clearAlert
  });

  const actions = React.useMemo(
    () => ({
      onSnap: snapTile,
      onClose: requestCloseTile,
      onReveal: revealTilePath,
      onLink: linkNoteTo,
      onCwd: setTileCwd,
      onMove: moveTile,
      onRename: setTileTitle,
      onResize: resizeTile,
      onActivate: activateAndClear,
      onCopyNote: copyNote,
      onCopyPath: copyTilePath,
      onPasteNote: pasteNote,
      onNoteTitle: setNoteTitle,
      onOscTitle: setTileOscTitle,
      onDuplicate: duplicateTile,
      onFocusTile: focusTile,
      onTogglePin: togglePin,
      onToggleRaw: toggleNoteRaw,
      onMoveToTab: moveTileToTabWrapper,
      onPatchOrchTile: patchOrchTile,
      onNoteChange: setNoteContent,
      onNoteEditor: registerEditor,
      onAgentState: setAgentState,
      onLinkDragStart: startLinkDrag,
      onOpenRunOutput: addRunView,
      onToggleSelect: toggleSelect,
      onToggleFullscreen: toggleFs,
      onCopyNoteSelection: copyNoteSelection,
      unlinkTerm: unlinkTermFrom,
      unlinkNote: unlinkNoteFrom
    }),
    [
      snapTile, requestCloseTile, revealTilePath, linkNoteTo, setTileCwd, moveTile,
      setTileTitle, resizeTile, activateAndClear, copyNote, copyTilePath, pasteNote,
      setNoteTitle, setTileOscTitle, duplicateTile, focusTile, togglePin, toggleNoteRaw,
      moveTileToTabWrapper, patchOrchTile, setNoteContent, registerEditor, setAgentState,
      startLinkDrag, addRunView, toggleSelect, toggleFs, copyNoteSelection,
      unlinkTermFrom, unlinkNoteFrom
    ]
  );

  const tileHostValue = React.useMemo(
    () => ({ fsBox, tabs, exiting: fsExit, wsId: activeId, actions, activeTabId, boards }),
    [fsBox, tabs, fsExit, activeId, actions, activeTabId, boards]
  );

  return (
    <div className={fsId ? `${styles.root} ${styles.rootFs}` : styles.root} style={benchOffset}>
      <div
        ref={bgRef}
        style={bgStyle}
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
        {backdrop && !fsId && (
          <img alt="" draggable={false} src={backdrop.url} className={styles.backdrop} style={{ opacity: backdrop.opacity }} />
        )}
        <canvas ref={gridRef} className={styles.grid} />
        {!fsId && (
          <svg className={styles.links} width={size.w} height={size.h}>
            {tiles.map((t) => {
              if ((t.type !== 'note' && t.type !== 'term') || !t.linkedTo?.length) return null;
              const srcRect = {
                x: t.x * view.k + view.x,
                y: t.y * view.k + view.y,
                width: t.width * view.k,
                height: t.height * view.k
              };
              return t.linkedTo.map((termId) => {
                const term = tiles.find((x) => x.id === termId);
                if (!term) return null;
                const termRect = {
                  x: term.x * view.k + view.x,
                  y: term.y * view.k + view.y,
                  width: term.width * view.k,
                  height: term.height * view.k
                };
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
                const srcRect = {
                  x: src.x * view.k + view.x,
                  y: src.y * view.k + view.y,
                  width: src.width * view.k,
                  height: src.height * view.k
                };
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
        {!fsId && frames.map((f) => <Frame key={f.id} frame={f} view={view} onSnap={snapFrame} onResize={resizeFrame} />)}
        <TileHostProvider value={tileHostValue}>
          {tiles.map((t) => {
            const vis = isVisible(t);
            if (vis && (t.width - TILE_GAP) * view.k >= MIN_LIVE_WIDTH) activated.current.add(t.id);
            const live = activated.current.has(t.id);
            const linkedIds =
              t.type === 'note'
                ? (t.linkedTo ?? [])
                : t.type === 'term' && !t.runCwd
                  ? [
                      ...new Set([
                        ...(t.linkedTo ?? []),
                        ...tiles.filter((x) => x.type === 'term' && (x.linkedTo ?? []).includes(t.id)).map((x) => x.id)
                      ])
                    ]
                  : [];
            const linkActive = t.type === 'note' && linkedIds.some((id) => id === activeTile || selected.has(id));
            const cand = t.type === 'note' ? adjacentTerm(t, tiles) : null;
            const linkCand = cand && !linkedIds.includes(cand.id) ? cand : null;
            const linkedTerms = linkedIds
              .map((id) => tiles.find((x) => x.id === id))
              .filter((x): x is (typeof tiles)[number] => Boolean(x))
              .map((term) => ({ id: term.id, name: termName(term) }));
            return (
              <TileFrame
                key={t.id}
                tile={t}
                view={view}
                status={{
                  live,
                  visible: vis,
                  fullscreen: t.id === fsId,
                  active: t.id === activeTile,
                  selected: selected.has(t.id),
                  alert: alertKinds.get(t.id) ?? null,
                  hidden: fsId !== null && t.id !== fsId
                }}
                link={{
                  linkedTerms,
                  active: linkActive,
                  target: linkCand ? { id: linkCand.id, name: termName(linkCand) } : null
                }}
              />
            );
          })}
        </TileHostProvider>
        {!fsId &&
          frames.map((f) => (
            <FrameBar
              key={f.id}
              frame={f}
              view={view}
              tiles={tiles}
              recede={receded.has(f.id)}
              board={boardPropsForFrame(f)}
              onFit={fitFrame}
              onReorder={reorderFrame}
              onDrag={dragFrame}
              onDragEnd={settleFrameDrag}
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
        {!fsId && <Minimap view={view} tiles={tiles} agents={agents} alerts={alertKinds} viewportRef={bgRef} onPan={panTo} />}
      </div>
      <Toasts />
      {commandBarOpen && <CommandBar sources={commandBarSources} onClose={closeCommandBar} />}
      {navOpen && leftHasVisible && (
        <CanvasDock
          mode={sidebarMode}
          side="left"
          tiles={tiles}
          groups={canvasGroups}
          alerts={alertKinds}
          agents={agents}
          panels={leftPanels}
          activeTile={activeTile}
          activeDiff={activeDiffFile}
          activeGroupKey={activeGroupKey}
          recentProjects={[]}
          handlers={leftDockHandlers}
          projectActions={projectActionsWithBoards}
        />
      )}
      {dockRightOpen && rightHasVisible && (
        <CanvasDock
          mode={sidebarMode}
          side="right"
          tiles={tiles}
          groups={canvasGroups}
          alerts={alertKinds}
          agents={agents}
          panels={rightPanels}
          activeTile={activeTile}
          activeDiff={activeDiffFile}
          activeGroupKey={activeGroupKey}
          recentProjects={[]}
          handlers={rightDockHandlers}
          projectActions={projectActionsWithBoards}
        />
      )}
      {splitting && <SplitHandle left={9 + navInset + fsWidth} onMove={moveSplit} />}
      {activeEditorTab && (
        <Workbench
          tabs={editorTabs}
          active={activeEditorTab}
          exiting={tabsExit}
          diffFiles={activeDiffTab?.base ? refDiffFiles : diffFiles}
          handlers={workbenchHandlers}
        />
      )}
      {tileToClose && (
        <UnsavedDialog paths={[tileToClose.path]} onSave={onSaveTile} onDiscard={discardTile} onCancel={cancelCloseTile} />
      )}
      {!navOpen && leftHasVisible && (
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
      {!dockRightOpen && rightHasVisible && (
        <div className={styles.dockRightRevealZone}>
          <button
            className={styles.dockRightReveal}
            onClick={showDockRight}
            data-tooltip="Show right panel"
            data-shortcut={formatCombo(getBinding('view.inspector'))}
            aria-label="Show right panel"
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
      {menu && (
        <ContextMenu
          x={menu.sx}
          y={menu.sy}
          onClose={closeMenu}
          items={[
            { label: 'New terminal', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: newTerminal },
            { label: 'New note', icon: <StickyNote size={15} strokeWidth={1.75} />, onSelect: newNote },
            { label: 'New frame', icon: <SquareDashed size={15} strokeWidth={1.75} />, onSelect: newFrame },
            { label: 'Reorder canvas', icon: <LayoutGrid size={15} strokeWidth={1.75} />, onSelect: reorderCanvas },
            ...(selected.size
              ? [{ label: 'Frame selection', icon: <Group size={15} strokeWidth={1.75} />, onSelect: frameSelection }]
              : [])
          ]}
        />
      )}
      {addProjectOpen && (
        <AddProjectModal
          recentProjects={[]}
          existingProjects={canvasProjects}
          onSelect={handleFolderPickerSelect}
          onClose={handleFolderPickerClose}
        />
      )}
      {newBoard && (
        <NewBoardDialog
          repo={newBoard.repo}
          onClose={() => setNewBoard(null)}
          onCreated={handleNewBoardCreated}
        />
      )}
    </div>
  );
};

export default Canvas;
