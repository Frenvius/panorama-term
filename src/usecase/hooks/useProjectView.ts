import type { MoveTileOpts } from '~/usecase/util/projectLayout';
import type { RuntimeCanvas } from '~/usecase/util/workspaceCanvas';
import type { CanvasState } from '~/domain/interfaces/workspace.interface';
import type { Tile, View, Frame } from '~/domain/interfaces/canvas.interface';
import type { SplitDirection, ProjectViewState, ProjectPanelState } from '~/domain/interfaces/projectView.interface';

import React from 'react';
import { listen } from '@tauri-apps/api/event';

import { clearAlert } from '~/usecase/util/alerts';
import { NOTE_DEFAULT_COLOR } from '~/usecase/util/note';
import { getPref, setPref } from '~/usecase/util/prefs';
import { touchTile } from '~/usecase/util/tileRecency';
import { killPtySession } from '~/adapter/pty/sidecar.client';
import { toStored, toRuntime } from '~/usecase/util/workspaceCanvas';
import { workspaceService } from '~/usecase/service/workspace.service';
import { unlinkNote, deleteNote, unlinkTerm } from '~/adapter/notes/notes.client';
import { withFrameCreated, withFramePatched } from '~/usecase/util/frame';
import { CELL, TILE_WIDTH, NOTE_WIDTH, NOTE_HEIGHT, TILE_HEIGHT } from '~/usecase/util/constants';
import {
  tileProject,
  closePanelGroup,
  DEFAULT_PROJECT,
  splitPanelGroup,
  updateSplitRatio,
  tileMatchesProject,
  normalizeProjectPath,
  activateTileInProject,
  moveTileBetweenGroups,
  emptyProjectPanelState,
  syncProjectViewWithTiles,
  removeTileFromPanelState
} from '~/usecase/util/projectLayout';
import { buildProjectGroups, groupPathResolver } from '~/usecase/util/projectGroups';

const createId = (): string => Math.random().toString(36).slice(2, 10);

const syncWithFrames = (view: ProjectViewState | undefined, tiles: Tile[], frames: Frame[]): ProjectViewState =>
  syncProjectViewWithTiles(view, tiles, groupPathResolver(buildProjectGroups({ frames, tiles, projects: view?.projects ?? [] })));

const CLOSED_KEY = 'panorama:closedTiles';
const CLOSED_MAX = 20;
const RECENT_KEY = 'panorama:recentProjects';
const RECENT_MAX = 12;

const DEFAULT_LEFT_WIDTH = 240;
const DEFAULT_RIGHT_WIDTH = 300;
const MIN_LEFT_WIDTH = 180;
const MAX_LEFT_WIDTH = 420;
const MIN_RIGHT_WIDTH = 220;
const MAX_RIGHT_WIDTH = 560;

const EMPTY: RuntimeCanvas = { tiles: [], frames: [], view: { x: 0, y: 0, k: 1 } };

const loadClosed = (ws: string | null): Tile[] => {
  if (!ws) return [];
  try {
    return JSON.parse(getPref(`${CLOSED_KEY}:${ws}`) || '[]');
  } catch {
    return [];
  }
};

const saveClosed = (ws: string | null, stack: Tile[]): void => {
  if (!ws) return;
  try {
    setPref(`${CLOSED_KEY}:${ws}`, JSON.stringify(stack.slice(-CLOSED_MAX)));
  } catch {}
};

const dedupeProjectPaths = (paths: (string | undefined | null)[]): string[] => {
  const result: string[] = [];
  const seen = new Set<string>();
  for (const raw of paths) {
    if (!raw) continue;
    const norm = normalizeProjectPath(raw);
    if (!norm) continue;
    const key = norm.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    result.push(norm);
    if (result.length >= RECENT_MAX) break;
  }
  return result;
};

const loadRecentProjects = (): string[] => {
  try {
    const raw = JSON.parse(getPref(RECENT_KEY) || '[]');
    if (!Array.isArray(raw)) return [];
    return dedupeProjectPaths(raw.filter((item): item is string => typeof item === 'string'));
  } catch {
    return [];
  }
};

const saveRecentProjects = (projects: string[]): void => {
  try {
    setPref(RECENT_KEY, JSON.stringify(projects.slice(0, RECENT_MAX)));
  } catch {}
};

const nextTileSlot = (existing: Tile[], width: number, height: number): { x: number; y: number } => {
  const originX = -Math.round(TILE_WIDTH / 2 / CELL) * CELL;
  const originY = -Math.round(TILE_HEIGHT / 2 / CELL) * CELL;
  const stepX = Math.ceil((TILE_WIDTH + CELL * 2) / CELL) * CELL;
  const stepY = Math.ceil((TILE_HEIGHT + CELL * 2) / CELL) * CELL;
  for (let i = 0; i < existing.length + 1; i++) {
    const col = i % 3;
    const row = Math.floor(i / 3);
    const x = originX + col * stepX;
    const y = originY + row * stepY;
    const overlaps = existing.some((t) => x < t.x + t.width && x + width > t.x && y < t.y + t.height && y + height > t.y);
    if (!overlaps) return { x, y };
  }
  return { x: originX, y: originY };
};

export interface UseProjectViewArgs {
  seed: CanvasState | null;
  wsId: string | null;
  onPersist: (state: CanvasState) => void;
}

export const useProjectView = ({ seed, wsId, onPersist }: UseProjectViewArgs) => {
  const wsIdRef = React.useRef(wsId);
  wsIdRef.current = wsId;

  const initial = React.useRef(seed ? toRuntime(seed) : EMPTY);
  const [frames, setFrames] = React.useState<Frame[]>(initial.current.frames);
  const [view] = React.useState<View>(initial.current.view);
  const [tiles, setTiles] = React.useState<Tile[]>(initial.current.tiles);
  const [projectView, setProjectView] = React.useState<ProjectViewState>(() =>
    syncWithFrames(seed?.projectView, initial.current.tiles, initial.current.frames)
  );
  const [recentProjects, setRecentProjects] = React.useState<string[]>(() =>
    dedupeProjectPaths([...(seed?.projectView?.projects ?? []), ...loadRecentProjects()])
  );
  const [fallbackActiveTileId, setFallbackActiveTileId] = React.useState<string | null>(null);

  const viewRef = React.useRef(view);
  viewRef.current = view;
  const framesRef = React.useRef(frames);
  framesRef.current = frames;
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;
  const projectViewRef = React.useRef(projectView);
  projectViewRef.current = projectView;

  const syncedFrames = React.useRef(frames);
  React.useEffect(() => {
    if (syncedFrames.current === frames) return;
    syncedFrames.current = frames;
    setProjectView((prev) => {
      const next = syncWithFrames(prev, tilesRef.current, frames);
      projectViewRef.current = next;
      return next;
    });
  }, [frames]);

  const pushRecentProjects = React.useCallback((paths: (string | undefined | null)[]) => {
    setRecentProjects((prev) => {
      const next = dedupeProjectPaths([...paths, ...prev]);
      if (next.length === prev.length && next.every((p, idx) => p === prev[idx])) return prev;
      saveRecentProjects(next);
      return next;
    });
  }, []);

  React.useEffect(() => {
    if (projectView.projects.length > 0) {
      pushRecentProjects([projectView.activeProject, ...projectView.projects]);
    }
  }, [projectView.projects, projectView.activeProject, pushRecentProjects]);

  React.useEffect(() => {
    let cancelled = false;
    const discover = async () => {
      try {
        const res = await workspaceService.list();
        const found: string[] = [];
        for (const ws of res.workspaces) {
          const tabs = await workspaceService.peekTabs(ws.id);
          for (const tab of tabs) {
            for (const p of tab.state.projectView?.projects ?? []) found.push(p);
            for (const t of tab.state.tiles ?? []) {
              const candidate = t.workspacePath ?? t.folderPath ?? t.cwd;
              if (candidate) found.push(candidate);
            }
          }
        }
        if (cancelled || found.length === 0) return;
        setRecentProjects((prev) => {
          const next = dedupeProjectPaths([...prev, ...found]);
          if (next.length === prev.length && next.every((p, idx) => p === prev[idx])) return prev;
          saveRecentProjects(next);
          return next;
        });
      } catch {}
    };
    void discover();
    return () => {
      cancelled = true;
    };
  }, []);

  const onPersistRef = React.useRef(onPersist);
  onPersistRef.current = onPersist;
  const pendingSaveRef = React.useRef(false);

  React.useEffect(() => {
    pendingSaveRef.current = true;
    const id = setTimeout(() => {
      pendingSaveRef.current = false;
      onPersistRef.current(
        toStored({
          tiles: tilesRef.current,
          view: viewRef.current,
          frames: framesRef.current,
          projectView: projectViewRef.current
        })
      );
    }, 400);
    return () => clearTimeout(id);
  }, [tiles, view, frames, projectView]);

  React.useEffect(
    () => () => {
      if (!pendingSaveRef.current) return;
      pendingSaveRef.current = false;
      onPersistRef.current(
        toStored({
          tiles: tilesRef.current,
          view: viewRef.current,
          frames: framesRef.current,
          projectView: projectViewRef.current
        })
      );
    },
    []
  );

  React.useEffect(() => {
    const off = listen<{ noteId: string; content: string }>('note:changed', (e) => {
      const { noteId, content } = e.payload;
      setTiles((prev) => {
        const next = prev.map((t) => (t.id === noteId && t.type === 'note' && t.content !== content ? { ...t, content } : t));
        tilesRef.current = next;
        return next;
      });
    });
    return () => {
      void off.then((un) => un());
    };
  }, []);

  const bindKey = React.useCallback((id: string) => {
    const tile = tilesRef.current.find((t) => t.id === id);
    return tile?.ptySessionId ?? id;
  }, []);

  const activeProject = projectView.activeProject;
  const panelState: ProjectPanelState | null = activeProject ? (projectView.byProject[activeProject] ?? null) : null;

  const activeTileId = React.useMemo(() => {
    if (panelState) {
      const activeGroup = panelState.groups[panelState.activeGroupId];
      if (activeGroup?.activeTileId) return activeGroup.activeTileId;
      for (const g of Object.values(panelState.groups)) {
        if (g.activeTileId) return g.activeTileId;
      }
      return null;
    }
    if (fallbackActiveTileId && tiles.some((t) => t.id === fallbackActiveTileId)) {
      return fallbackActiveTileId;
    }
    return tiles[tiles.length - 1]?.id ?? null;
  }, [panelState, fallbackActiveTileId, tiles]);

  React.useEffect(() => {
    if (!activeTileId) return;
    clearAlert(activeTileId);
    touchTile(activeTileId);
  }, [activeTileId]);

  const selectProject = React.useCallback(
    (path: string) => {
      const norm = normalizeProjectPath(path);
      if (!norm) return;
      pushRecentProjects([norm]);
      setProjectView((prev) => {
        const existing = prev.projects.find((p) => p.toLowerCase() === norm.toLowerCase());
        const canonical = existing ?? norm;
        const nextProjects = existing ? prev.projects : [...prev.projects, canonical];
        const matchingIds = tilesRef.current.filter((t) => tileMatchesProject(t, canonical, nextProjects)).map((t) => t.id);
        const panel = prev.byProject[canonical] ?? emptyProjectPanelState(matchingIds);
        const next: ProjectViewState = {
          ...prev,
          projects: nextProjects,
          activeProject: canonical,
          byProject: {
            ...prev.byProject,
            [canonical]: panel
          }
        };
        projectViewRef.current = next;
        return next;
      });
    },
    [pushRecentProjects]
  );

  const addProject = React.useCallback(
    (path: string) => {
      const norm = normalizeProjectPath(path);
      if (!norm) return;
      pushRecentProjects([norm]);

      const currentPv = projectViewRef.current;
      const existing = currentPv.projects.find((p) => p.toLowerCase() === norm.toLowerCase());
      const canonical = existing ?? norm;
      const nextProjects = existing ? currentPv.projects : [...currentPv.projects, canonical];
      const existingMatching = tilesRef.current.filter((t) => tileMatchesProject(t, canonical, nextProjects));

      let spawnedTileId: string | null = null;
      if (existingMatching.length === 0) {
        const id = createId();
        spawnedTileId = id;
        const prevTiles = tilesRef.current;
        const slot = nextTileSlot(prevTiles, TILE_WIDTH, TILE_HEIGHT);
        const zIndex = prevTiles.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
        const newTile: Tile = {
          id,
          type: 'term',
          cwd: canonical,
          workspacePath: canonical,
          autoTitle: canonical,
          x: slot.x,
          y: slot.y,
          width: TILE_WIDTH,
          height: TILE_HEIGHT,
          zIndex
        };
        const nextTiles = [...prevTiles, newTile];
        tilesRef.current = nextTiles;
        setTiles(nextTiles);
      }

      setProjectView((prev) => {
        const prevExisting = prev.projects.find((p) => p.toLowerCase() === norm.toLowerCase());
        const proj = prevExisting ?? norm;
        const projects = prevExisting ? prev.projects : [...prev.projects, proj];
        const baseIds = spawnedTileId ? [spawnedTileId] : existingMatching.map((t) => t.id);
        const basePanel = prev.byProject[proj] ?? emptyProjectPanelState(baseIds);
        const nextPanel = spawnedTileId ? activateTileInProject(basePanel, { tileId: spawnedTileId }) : basePanel;
        const next: ProjectViewState = {
          ...prev,
          projects,
          activeProject: proj,
          byProject: {
            ...prev.byProject,
            [proj]: nextPanel
          }
        };
        projectViewRef.current = next;
        return next;
      });
    },
    [pushRecentProjects]
  );

  const removeProject = React.useCallback((path: string) => {
    const norm = normalizeProjectPath(path);
    if (!norm) return;
    const key = norm.toLowerCase();

    setTiles((prev) => {
      const next = prev.map((t) =>
        t.workspacePath && normalizeProjectPath(t.workspacePath).toLowerCase() === key ? { ...t, workspacePath: undefined } : t
      );
      tilesRef.current = next;
      return next;
    });

    setProjectView((prev) => {
      const nextProjects = prev.projects.filter((p) => p.toLowerCase() !== key);
      const nextByProject: Record<string, ProjectPanelState> = {};
      for (const [proj, panel] of Object.entries(prev.byProject)) {
        if (proj.toLowerCase() !== key) nextByProject[proj] = panel;
      }
      const nextActive =
        prev.activeProject && prev.activeProject.toLowerCase() !== key ? prev.activeProject : (nextProjects[0] ?? null);
      const next: ProjectViewState = {
        ...prev,
        projects: nextProjects,
        activeProject: nextActive,
        byProject: nextByProject
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const focusGroup = React.useCallback((groupId: string) => {
    setProjectView((prev) => {
      const proj = prev.activeProject;
      if (!proj) return prev;
      const panel = prev.byProject[proj];
      if (!panel || !panel.groups[groupId] || panel.activeGroupId === groupId) return prev;
      const next: ProjectViewState = {
        ...prev,
        byProject: {
          ...prev.byProject,
          [proj]: {
            ...panel,
            activeGroupId: groupId
          }
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const activateTile = React.useCallback((tileId: string, groupId?: string) => {
    setFallbackActiveTileId(tileId);
    setProjectView((prev) => {
      const activeProj = prev.activeProject;
      const activePanel = activeProj ? prev.byProject[activeProj] : undefined;
      const inActiveProj =
        activePanel &&
        (Object.values(activePanel.groups).some((g) => g.tileIds.includes(tileId)) ||
          Boolean(groupId && activePanel.groups[groupId]));

      let targetProj: string | null = inActiveProj ? activeProj : null;
      if (!targetProj) {
        for (const [proj, panel] of Object.entries(prev.byProject)) {
          if (Object.values(panel.groups).some((g) => g.tileIds.includes(tileId))) {
            targetProj = proj;
            break;
          }
        }
      }
      if (!targetProj) {
        const tile = tilesRef.current.find((t) => t.id === tileId);
        if (tile) targetProj = tileProject(tile, prev.projects);
      }
      targetProj = targetProj ?? activeProj;
      if (!targetProj) return prev;

      const basePanel = prev.byProject[targetProj] ?? emptyProjectPanelState([tileId], groupId);
      const nextPanel = activateTileInProject(basePanel, { tileId, groupId });
      const next: ProjectViewState = {
        ...prev,
        activeProject: targetProj,
        byProject: {
          ...prev.byProject,
          [targetProj]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const splitGroup = React.useCallback((direction: SplitDirection, groupId?: string, spawnTerm?: boolean) => {
    const currentPv = projectViewRef.current;
    const activeProj = currentPv.activeProject;
    if (!activeProj) return;
    const panel = currentPv.byProject[activeProj] ?? emptyProjectPanelState();
    const targetGroupId = groupId ?? panel.activeGroupId;
    const targetGroup = panel.groups[targetGroupId];
    if (!targetGroup) return;

    const shouldSpawn = Boolean(spawnTerm) || targetGroup.tileIds.length <= 1;
    let spawnedTileId: string | undefined;

    if (shouldSpawn) {
      const id = createId();
      spawnedTileId = id;
      const prevTiles = tilesRef.current;
      const slot = nextTileSlot(prevTiles, TILE_WIDTH, TILE_HEIGHT);
      const zIndex = prevTiles.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
      const newTile: Tile = {
        id,
        type: 'term',
        cwd: activeProj,
        workspacePath: activeProj,
        autoTitle: activeProj,
        x: slot.x,
        y: slot.y,
        width: TILE_WIDTH,
        height: TILE_HEIGHT,
        zIndex
      };
      const nextTiles = [...prevTiles, newTile];
      tilesRef.current = nextTiles;
      setTiles(nextTiles);
    }

    setProjectView((prev) => {
      const proj = prev.activeProject;
      if (!proj) return prev;
      const currentPanel = prev.byProject[proj] ?? emptyProjectPanelState();
      const nextPanel = splitPanelGroup(currentPanel, {
        direction,
        groupId: targetGroupId,
        tileId: spawnedTileId
      });
      const next: ProjectViewState = {
        ...prev,
        byProject: {
          ...prev.byProject,
          [proj]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const moveTile = React.useCallback((opts: MoveTileOpts) => {
    setProjectView((prev) => {
      const proj = prev.activeProject;
      if (!proj) return prev;
      const panel = prev.byProject[proj];
      if (!panel) return prev;
      const nextPanel = moveTileBetweenGroups(panel, opts);
      const next: ProjectViewState = {
        ...prev,
        byProject: {
          ...prev.byProject,
          [proj]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const resizeSplit = React.useCallback((path: ('first' | 'second')[], ratio: number) => {
    setProjectView((prev) => {
      const proj = prev.activeProject;
      if (!proj) return prev;
      const panel = prev.byProject[proj];
      if (!panel) return prev;
      const nextLayout = updateSplitRatio(panel.layout, path, ratio);
      if (nextLayout === panel.layout) return prev;
      const next: ProjectViewState = {
        ...prev,
        byProject: {
          ...prev.byProject,
          [proj]: {
            ...panel,
            layout: nextLayout
          }
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const toggleMaximizeGroup = React.useCallback((groupId?: string) => {
    setProjectView((prev) => {
      const proj = prev.activeProject;
      if (!proj) return prev;
      const panel = prev.byProject[proj];
      if (!panel) return prev;
      const targetId = groupId ?? panel.activeGroupId;
      if (!panel.groups[targetId]) return prev;
      const nextMaximized = panel.maximizedGroupId === targetId ? null : targetId;
      const next: ProjectViewState = {
        ...prev,
        byProject: {
          ...prev.byProject,
          [proj]: {
            ...panel,
            activeGroupId: targetId,
            maximizedGroupId: nextMaximized
          }
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const closeGroup = React.useCallback((groupId: string) => {
    setProjectView((prev) => {
      const proj = prev.activeProject;
      if (!proj) return prev;
      const panel = prev.byProject[proj];
      if (!panel) return prev;
      const nextPanel = closePanelGroup(panel, groupId);
      const next: ProjectViewState = {
        ...prev,
        byProject: {
          ...prev.byProject,
          [proj]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const addTerminal = React.useCallback((projectPath?: string, groupId?: string, at?: { x: number; y: number }): string => {
    const rawTarget = projectPath ?? projectViewRef.current.activeProject ?? undefined;
    const targetProject = rawTarget ? normalizeProjectPath(rawTarget) : DEFAULT_PROJECT;
    const realPath = targetProject === DEFAULT_PROJECT ? undefined : targetProject;
    const id = createId();
    const prevTiles = tilesRef.current;
    const slot = at ?? nextTileSlot(prevTiles, TILE_WIDTH, TILE_HEIGHT);
    const zIndex = prevTiles.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
    const tile: Tile = {
      id,
      type: 'term',
      x: slot.x,
      y: slot.y,
      width: TILE_WIDTH,
      height: TILE_HEIGHT,
      zIndex,
      ...(realPath ? { cwd: realPath, workspacePath: realPath, autoTitle: realPath } : {})
    };
    const nextTiles = [...prevTiles, tile];
    tilesRef.current = nextTiles;
    setTiles(nextTiles);
    setFallbackActiveTileId(id);

    setProjectView((prev) => {
      const existing = prev.projects.find((p) => p.toLowerCase() === targetProject.toLowerCase());
      const canonical = existing ?? targetProject;
      const projects = existing ? prev.projects : [...prev.projects, canonical];
      const basePanel = prev.byProject[canonical] ?? emptyProjectPanelState([id], groupId);
      const nextPanel = activateTileInProject(basePanel, { tileId: id, groupId });
      const next: ProjectViewState = {
        ...prev,
        projects,
        activeProject: canonical,
        byProject: {
          ...prev.byProject,
          [canonical]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });

    return id;
  }, []);

  const addNote = React.useCallback((projectPath?: string, groupId?: string, at?: { x: number; y: number }): string => {
    const rawTarget = projectPath ?? projectViewRef.current.activeProject ?? undefined;
    const targetProject = rawTarget ? normalizeProjectPath(rawTarget) : DEFAULT_PROJECT;
    const realPath = targetProject === DEFAULT_PROJECT ? undefined : targetProject;
    const id = createId();
    const prevTiles = tilesRef.current;
    const slot = at ?? nextTileSlot(prevTiles, NOTE_WIDTH, NOTE_HEIGHT);
    const zIndex = prevTiles.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
    const tile: Tile = {
      id,
      type: 'note',
      x: slot.x,
      y: slot.y,
      width: NOTE_WIDTH,
      height: NOTE_HEIGHT,
      color: NOTE_DEFAULT_COLOR,
      content: '',
      zIndex,
      ...(realPath ? { folderPath: realPath, workspacePath: realPath } : {})
    };
    const nextTiles = [...prevTiles, tile];
    tilesRef.current = nextTiles;
    setTiles(nextTiles);
    setFallbackActiveTileId(id);

    setProjectView((prev) => {
      const existing = prev.projects.find((p) => p.toLowerCase() === targetProject.toLowerCase());
      const canonical = existing ?? targetProject;
      const projects = existing ? prev.projects : [...prev.projects, canonical];
      const basePanel = prev.byProject[canonical] ?? emptyProjectPanelState([id], groupId);
      const nextPanel = activateTileInProject(basePanel, { tileId: id, groupId });
      const next: ProjectViewState = {
        ...prev,
        projects,
        activeProject: canonical,
        byProject: {
          ...prev.byProject,
          [canonical]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });

    return id;
  }, []);

  const placeFileTile = React.useCallback(
    (root: string, filePath: string, type: 'editor' | 'code', sideSplit?: boolean) => {
      const normRoot = normalizeProjectPath(root) || projectViewRef.current.activeProject || '';
      const existingTile = tilesRef.current.find((t) => {
        if (t.type !== type || t.filePath !== filePath) return false;
        if (type === 'code' && t.cwd) {
          return normalizeProjectPath(t.cwd).toLowerCase() === normalizeProjectPath(root).toLowerCase();
        }
        return true;
      });

      if (existingTile) {
        if (sideSplit && normRoot) {
          setProjectView((prev) => {
            const canonical =
              prev.projects.find((p) => p.toLowerCase() === normRoot.toLowerCase()) ?? prev.activeProject ?? normRoot;
            const basePanel = prev.byProject[canonical] ?? emptyProjectPanelState([existingTile.id]);
            const nextPanel = splitPanelGroup(basePanel, {
              direction: 'horizontal',
              tileId: existingTile.id
            });
            const next: ProjectViewState = {
              ...prev,
              activeProject: canonical,
              byProject: {
                ...prev.byProject,
                [canonical]: nextPanel
              }
            };
            projectViewRef.current = next;
            return next;
          });
        } else {
          activateTile(existingTile.id);
        }
        return;
      }

      const id = createId();
      const prevTiles = tilesRef.current;
      const slot = nextTileSlot(prevTiles, TILE_WIDTH, TILE_HEIGHT);
      const zIndex = prevTiles.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
      const newTile: Tile = {
        id,
        type,
        cwd: root,
        filePath,
        workspacePath: normRoot || undefined,
        autoTitle: filePath.split(/[\\/]/).pop(),
        x: slot.x,
        y: slot.y,
        width: TILE_WIDTH,
        height: TILE_HEIGHT,
        zIndex
      };
      const nextTiles = [...prevTiles, newTile];
      tilesRef.current = nextTiles;
      setTiles(nextTiles);
      setFallbackActiveTileId(id);

      if (normRoot) {
        setProjectView((prev) => {
          const existingProj = prev.projects.find((p) => p.toLowerCase() === normRoot.toLowerCase());
          const canonical = existingProj ?? normRoot;
          const projects = existingProj ? prev.projects : [...prev.projects, canonical];
          const basePanel = prev.byProject[canonical] ?? emptyProjectPanelState();
          const activeGroup = basePanel.groups[basePanel.activeGroupId];
          const nextPanel =
            sideSplit && activeGroup && activeGroup.tileIds.length > 0
              ? splitPanelGroup(basePanel, { direction: 'horizontal', tileId: id })
              : activateTileInProject(basePanel, { tileId: id });
          const next: ProjectViewState = {
            ...prev,
            projects,
            activeProject: canonical,
            byProject: {
              ...prev.byProject,
              [canonical]: nextPanel
            }
          };
          projectViewRef.current = next;
          return next;
        });
      }
    },
    [activateTile]
  );

  const openFile = React.useCallback(
    (root: string, filePath: string, opts?: { preview?: boolean; sideSplit?: boolean }) => {
      placeFileTile(root, filePath, 'editor', opts?.sideSplit);
    },
    [placeFileTile]
  );

  const openDiff = React.useCallback(
    (root: string, filePath: string, opts?: { sideSplit?: boolean }) => {
      placeFileTile(root, filePath, 'code', opts?.sideSplit);
    },
    [placeFileTile]
  );

  const openRunOutput = React.useCallback(
    (srcId: string, cwd: string, sessionId: string, cmd: string) => {
      const existing = tilesRef.current.find((t) => t.runCwd && t.ptySessionId === sessionId);
      if (existing) {
        activateTile(existing.id);
        return;
      }
      const prevTiles = tilesRef.current;
      const src = prevTiles.find((t) => t.id === srcId);
      const targetProj =
        (src ? tileProject(src, projectViewRef.current.projects) : null) ??
        projectViewRef.current.activeProject ??
        normalizeProjectPath(cwd);
      const id = createId();
      const slot = nextTileSlot(prevTiles, TILE_WIDTH, TILE_HEIGHT);
      const zIndex = prevTiles.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
      const tile: Tile = {
        id,
        type: 'term',
        runCwd: cwd,
        ptySessionId: sessionId,
        cwd,
        workspacePath: targetProj || undefined,
        autoTitle: cmd,
        x: src ? src.x + src.width : slot.x,
        y: src ? src.y : slot.y,
        width: src?.width ?? TILE_WIDTH,
        height: src?.height ?? TILE_HEIGHT,
        zIndex
      };
      const nextTiles = [...prevTiles, tile];
      tilesRef.current = nextTiles;
      setTiles(nextTiles);
      setFallbackActiveTileId(id);

      if (targetProj) {
        setProjectView((prev) => {
          const existingProj = prev.projects.find((p) => p.toLowerCase() === targetProj.toLowerCase());
          const canonical = existingProj ?? targetProj;
          const projects = existingProj ? prev.projects : [...prev.projects, canonical];
          const basePanel = prev.byProject[canonical] ?? emptyProjectPanelState();
          const srcGroup = Object.values(basePanel.groups).find((g) => g.tileIds.includes(srcId));
          const nextPanel =
            srcGroup && srcGroup.tileIds.length > 0
              ? splitPanelGroup(basePanel, {
                  direction: 'vertical',
                  groupId: srcGroup.id,
                  tileId: id
                })
              : activateTileInProject(basePanel, { tileId: id });
          const next: ProjectViewState = {
            ...prev,
            projects,
            activeProject: canonical,
            byProject: {
              ...prev.byProject,
              [canonical]: nextPanel
            }
          };
          projectViewRef.current = next;
          return next;
        });
      }
    },
    [activateTile]
  );

  const closeTile = React.useCallback(
    (id: string) => {
      clearAlert(id);
      const closing = tilesRef.current.find((t) => t.id === id);
      if (closing && !closing.runCwd) {
        const ws = wsIdRef.current;
        saveClosed(ws, [...loadClosed(ws), closing]);
      }
      if (closing?.type === 'note') {
        for (const termId of closing.linkedTo ?? []) void unlinkNote(id, bindKey(termId)).catch(() => {});
        if (wsIdRef.current) void deleteNote(wsIdRef.current, id).catch(() => {});
      }
      if (closing?.type === 'term') {
        for (const peerId of closing.linkedTo ?? []) void unlinkTerm(bindKey(id), bindKey(peerId)).catch(() => {});
        for (const t of tilesRef.current) {
          if (!(t.linkedTo ?? []).includes(id)) continue;
          if (t.type === 'note') void unlinkNote(t.id, bindKey(id)).catch(() => {});
          if (t.type === 'term') void unlinkTerm(bindKey(t.id), bindKey(id)).catch(() => {});
        }
      }
      const nextTiles = tilesRef.current
        .filter((t) => t.id !== id)
        .map((t) => ((t.linkedTo ?? []).includes(id) ? { ...t, linkedTo: (t.linkedTo ?? []).filter((x) => x !== id) } : t));
      tilesRef.current = nextTiles;
      setTiles(nextTiles);
      setFallbackActiveTileId((a) => (a === id ? null : a));
      setProjectView((prev) => {
        const nextByProject: Record<string, ProjectPanelState> = {};
        for (const [proj, panel] of Object.entries(prev.byProject)) {
          nextByProject[proj] = removeTileFromPanelState(panel, id);
        }
        const next: ProjectViewState = {
          ...prev,
          byProject: nextByProject
        };
        projectViewRef.current = next;
        return next;
      });
      void killPtySession(id);
    },
    [bindKey]
  );

  const reopenTile = React.useCallback(() => {
    const ws = wsIdRef.current;
    const stack = loadClosed(ws);
    const tile = stack.pop();
    saveClosed(ws, stack);
    if (!tile) return;
    if (tilesRef.current.some((t) => t.id === tile.id)) return;
    const zIndex = tilesRef.current.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
    const activeProj = projectViewRef.current.activeProject;
    const restored: Tile = {
      ...tile,
      zIndex,
      ptySessionId: undefined,
      ...(activeProj && !tile.workspacePath ? { workspacePath: activeProj } : {})
    };
    const nextTiles = [...tilesRef.current, restored];
    tilesRef.current = nextTiles;
    setTiles(nextTiles);
    setFallbackActiveTileId(restored.id);

    setProjectView((prev) => {
      const proj =
        (restored.workspacePath ? normalizeProjectPath(restored.workspacePath) : null) ??
        tileProject(restored, prev.projects) ??
        prev.activeProject;
      if (!proj) {
        const next = syncWithFrames(prev, nextTiles, framesRef.current);
        projectViewRef.current = next;
        return next;
      }
      const existing = prev.projects.find((p) => p.toLowerCase() === proj.toLowerCase());
      const canonical = existing ?? proj;
      const projects = existing ? prev.projects : [...prev.projects, canonical];
      const basePanel = prev.byProject[canonical] ?? emptyProjectPanelState([restored.id]);
      const nextPanel = activateTileInProject(basePanel, { tileId: restored.id });
      const next: ProjectViewState = {
        ...prev,
        projects,
        activeProject: canonical,
        byProject: {
          ...prev.byProject,
          [canonical]: nextPanel
        }
      };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const patchTile = React.useCallback((id: string, patch: Partial<Tile>) => {
    setTiles((prev) => {
      const next = prev.map((t) => (t.id === id ? { ...t, ...patch } : t));
      tilesRef.current = next;
      return next;
    });
  }, []);

  const createFrame = React.useCallback((frame: Omit<Frame, 'id'>): string => {
    const id = Math.random().toString(36).slice(2, 10);
    setFrames((prev) => {
      const next = withFrameCreated(prev, frame, id);
      framesRef.current = next;
      return next;
    });
    return id;
  }, []);

  const patchFrame = React.useCallback((id: string, patch: Partial<Omit<Frame, 'id'>>) => {
    setFrames((prev) => {
      const next = withFramePatched(prev, id, patch);
      framesRef.current = next;
      return next;
    });
  }, []);

  const removeFrame = React.useCallback((id: string) => {
    setFrames((prev) => {
      const next = prev.filter((f) => f.id !== id);
      framesRef.current = next;
      return next;
    });
  }, []);

  const placeTiles = React.useCallback((moves: { id: string; x: number; y: number }[]) => {
    const posMap = new Map(moves.map((m) => [m.id, m]));
    setTiles((prev) => {
      const next = prev.map((t) => {
        const m = posMap.get(t.id);
        return m ? { ...t, x: m.x, y: m.y } : t;
      });
      tilesRef.current = next;
      return next;
    });
  }, []);

  const setTileCwd = React.useCallback((id: string, cwd: string, branch?: string) => {
    const autoTitle = cwd;
    const normCwd = normalizeProjectPath(cwd);
    const currentPv = projectViewRef.current;
    const hadNoRealProject =
      !currentPv.activeProject ||
      currentPv.activeProject === DEFAULT_PROJECT ||
      currentPv.projects.every((p) => p === DEFAULT_PROJECT);
    const nextTiles = tilesRef.current.map((t) => {
      if (t.id !== id) return t;
      const nextWorkspacePath =
        t.workspacePath && t.workspacePath !== DEFAULT_PROJECT
          ? t.workspacePath
          : hadNoRealProject && normCwd
            ? normCwd
            : t.workspacePath;
      if (t.cwd === cwd && t.autoTitle === autoTitle && t.branch === branch && t.workspacePath === nextWorkspacePath) {
        return t;
      }
      return {
        ...t,
        cwd,
        autoTitle,
        branch,
        ...(nextWorkspacePath ? { workspacePath: nextWorkspacePath } : {})
      };
    });
    tilesRef.current = nextTiles;
    setTiles(nextTiles);

    if (hadNoRealProject && normCwd && normCwd !== DEFAULT_PROJECT) {
      setProjectView((prev) => {
        const defaultPanel = prev.byProject[DEFAULT_PROJECT];
        const seededByProject = { ...prev.byProject };
        if (defaultPanel && !seededByProject[normCwd]) {
          seededByProject[normCwd] = defaultPanel;
        }
        delete seededByProject[DEFAULT_PROJECT];
        const next = syncWithFrames(
          {
            ...prev,
            projects: prev.projects.filter((p) => p !== DEFAULT_PROJECT),
            activeProject: normCwd,
            byProject: seededByProject
          },
          nextTiles,
          framesRef.current
        );
        projectViewRef.current = next;
        return next;
      });
    }
  }, []);

  const setTileOscTitle = React.useCallback((id: string, title: string) => {
    const oscTitle = title.trim() || undefined;
    setTiles((prev) => {
      const next = prev.map((t) => (t.id === id && t.oscTitle !== oscTitle ? { ...t, oscTitle } : t));
      tilesRef.current = next;
      return next;
    });
  }, []);

  const leftOpen = projectView.leftOpen ?? true;
  const leftWidth = projectView.leftWidth ?? DEFAULT_LEFT_WIDTH;
  const rightOpen = projectView.rightOpen ?? true;
  const rightWidth = projectView.rightWidth ?? DEFAULT_RIGHT_WIDTH;

  const setLeftOpen = React.useCallback((open: boolean) => {
    setProjectView((prev) => {
      if ((prev.leftOpen ?? true) === open) return prev;
      const next = { ...prev, leftOpen: open };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const setLeftWidth = React.useCallback((width: number) => {
    const clamped = Math.max(MIN_LEFT_WIDTH, Math.min(MAX_LEFT_WIDTH, Math.round(width)));
    setProjectView((prev) => {
      if ((prev.leftWidth ?? DEFAULT_LEFT_WIDTH) === clamped) return prev;
      const next = { ...prev, leftWidth: clamped };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const setRightOpen = React.useCallback((open: boolean) => {
    setProjectView((prev) => {
      if ((prev.rightOpen ?? true) === open) return prev;
      const next = { ...prev, rightOpen: open };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const setRightWidth = React.useCallback((width: number) => {
    const clamped = Math.max(MIN_RIGHT_WIDTH, Math.min(MAX_RIGHT_WIDTH, Math.round(width)));
    setProjectView((prev) => {
      if ((prev.rightWidth ?? DEFAULT_RIGHT_WIDTH) === clamped) return prev;
      const next = { ...prev, rightWidth: clamped };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const toggleLeftSidebar = React.useCallback(() => {
    setProjectView((prev) => {
      const next = { ...prev, leftOpen: !(prev.leftOpen ?? true) };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  const toggleRightSidebar = React.useCallback(() => {
    setProjectView((prev) => {
      const next = { ...prev, rightOpen: !(prev.rightOpen ?? true) };
      projectViewRef.current = next;
      return next;
    });
  }, []);

  return {
    view,
    tiles,
    frames,
    projectView,
    projects: projectView.projects,
    activeProject,
    recentProjects,
    selectProject,
    addProject,
    removeProject,
    panelState,
    activeTileId,
    focusGroup,
    activateTile,
    splitGroup,
    moveTile,
    resizeSplit,
    toggleMaximizeGroup,
    closeGroup,
    createFrame,
    patchFrame,
    removeFrame,
    placeTiles,
    addTerminal,
    addNote,
    openFile,
    openDiff,
    openRunOutput,
    closeTile,
    reopenTile,
    patchTile,
    setTileCwd,
    setTileOscTitle,
    leftOpen,
    leftWidth,
    rightOpen,
    rightWidth,
    setLeftOpen,
    setLeftWidth,
    setRightOpen,
    setRightWidth,
    toggleLeftSidebar,
    toggleRightSidebar
  };
};
