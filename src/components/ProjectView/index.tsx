import type { EditorView } from '@codemirror/view';
import type { PanelId } from '~/domain/interfaces/dock.interface';
import type { CommandId } from '~/usecase/util/keybindings';
import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { DropZone } from '~/domain/interfaces/projectView.interface';
import type { CommandBarSources } from '~/usecase/hooks/useCommandBarItems';
import type { NotifyKind, NotifyTarget } from '~/domain/interfaces/notify.interface';
import type { DraggingTabState, PanelSplitTreeProps } from './types';

import React from 'react';
import { FolderGit2, FolderPlus, LayoutGrid, SquareTerminal } from 'lucide-react';

import styles from './styles.module.scss';
import GitPanel from './GitPanel';
import DockerPanel from './DockerPanel';
import FilesPanel from './FilesPanel';
import NotesPanel from './NotesPanel';
import DockSidebar from './DockSidebar';
import PanelSplitTree from './PanelSplitTree';
import ProjectsPanel from '~/components/commons/ProjectsPanel';
import AddProjectModal from './AddProjectModal';
import Toasts from '~/components/commons/Toasts';
import DockRail from '~/components/commons/Dock/DockRail';
import { PANEL_DESCRIPTORS } from '~/components/commons/Dock/panels';
import { fileKey } from '~/usecase/util/fileTree';
import { onRefDiff } from '~/usecase/util/refDiff';
import CommandBar from '~/components/Canvas/CommandBar';
import { writeNote } from '~/adapter/notes/notes.client';
import NoteToolbar from '~/components/Canvas/NoteToolbar';
import { applyFrontTitle } from '~/usecase/util/noteMeta';
import UnsavedDialog from '~/components/commons/UnsavedDialog';
import { useDockLayout } from '~/usecase/hooks/useDockLayout';
import { useProjectView } from '~/usecase/hooks/useProjectView';
import { useRemoteTiles } from '~/usecase/hooks/useRemoteTiles';
import { visiblePanels } from '~/usecase/util/dockLayout';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';
import { dockerAvailable } from '~/adapter/docker/docker.client';
import { workspaceService } from '~/usecase/service/workspace.service';
import { useNotifyBridge } from '~/components/commons/Notifications/bridge';
import { setAlert, getAlerts, clearAlert, subscribeAlerts } from '~/usecase/util/alerts';
import { buildProjectGroups } from '~/usecase/util/projectGroups';
import { useProjectsPanelActions } from '~/usecase/hooks/useProjectsPanelActions';
import { projectName, DEFAULT_PROJECT, tileMatchesProject } from '~/usecase/util/projectLayout';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';
import { getBinding, isCapturing, formatCombo, matchCommand } from '~/usecase/util/keybindings';
import { setAgentStatus, getAgentStatuses, subscribeAgentStatuses } from '~/usecase/util/agentStatus';
import { isDirty, requestFind, requestSave, dispatchSave, requestPreview } from '~/usecase/util/dirtyFiles';
import { type ShowWorkerRequest, onOrchBoardRequest, onOrchDiffRequest, onShowWorkerRequest, getProjectLastBoard } from '~/usecase/util/orchBoardRequest';
import { coordinatorActions } from '~/usecase/hooks/useOrchCoordinator';

let pendingFocusTileId: string | null = null;

const jumpToWorkspaceTab = async (wsId: string, tabId: string, switchWorkspace: (id: string) => Promise<void>): Promise<void> => {
  await workspaceService.setActiveTab(wsId, tabId);
  await switchWorkspace(wsId);
};

const ProjectView = () => {
  const { tabs, activeId, switchTab, setTabMode, workspaces, activeState, activeTabId, switchWorkspace, saveActiveState } =
    useWorkspace();

  const {
    tiles,
    frames,
    projects,
    projectView,
    panelState,
    activeTileId,
    activeProject,
    recentProjects,
    selectProject,
    addProject,
    removeProject,
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
    detachTile,
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
    toggleRightSidebar,
    addOrchBoard
  } = useProjectView({ seed: activeState, wsId: activeId, onPersist: saveActiveState });

  const { layout, reset } = useDockLayout('project');

  const alerts = React.useSyncExternalStore(subscribeAlerts, getAlerts);
  const agents = React.useSyncExternalStore(subscribeAgentStatuses, getAgentStatuses);

  const handleShowWorkerRef = React.useRef<(req: ShowWorkerRequest) => void>(() => {});

  React.useEffect(() => {
    const unsubBoard = onOrchBoardRequest(({ runId }) => addOrchBoard(runId));
    const unsubWorker = onShowWorkerRequest((req) => handleShowWorkerRef.current(req));
    const unsubDiff = onOrchDiffRequest(({ root, file }) => openDiff(root, file));
    return () => { unsubBoard(); unsubWorker(); unsubDiff(); };
  }, [addOrchBoard, openDiff]);

  const [hasDocker, setHasDocker] = React.useState(false);
  const [draggingTab, setDraggingTab] = React.useState<DraggingTabState | null>(null);
  const [commandBarOpen, setCommandBarOpen] = React.useState(false);
  const [addProjectOpen, setAddProjectOpen] = React.useState(false);
  const [tileToClose, setTileToClose] = React.useState<{ id: string; path: string } | null>(null);
  const [noteEditors, setNoteEditors] = React.useState<Record<string, EditorView>>({});
  const [, setDiffFiles] = React.useState<string[]>([]);

  React.useEffect(() => {
    dockerAvailable()
      .then(setHasDocker)
      .catch(() => setHasDocker(false));
  }, []);

  const panelAvailable = React.useCallback((id: PanelId) => id !== 'docker' || hasDocker, [hasDocker]);

  const leftPanels = visiblePanels(layout, 'left', panelAvailable).map((id) => PANEL_DESCRIPTORS[id]);
  const rightPanels = visiblePanels(layout, 'right', panelAvailable).map((id) => PANEL_DESCRIPTORS[id]);

  const leftHasVisible = leftPanels.length > 0;
  const rightHasVisible = rightPanels.length > 0;

  const commandBarRef = React.useRef(commandBarOpen);
  commandBarRef.current = commandBarOpen;
  const activeTileIdRef = React.useRef(activeTileId);
  activeTileIdRef.current = activeTileId;
  const closeTileRef = React.useRef<(id: string) => void>(() => {});

  React.useLayoutEffect(() => {
    const id = pendingFocusTileId;
    if (!id || !tiles.some((t) => t.id === id)) return;
    pendingFocusTileId = null;
    clearAlert(id);
    activateTile(id);
  }, [tiles, activeTabId, activateTile]);

  const openRemote = React.useCallback(
    (id: string, cwd: string) => {
      if (tiles.some((t) => t.id === id)) {
        activateTile(id);
        return;
      }
      addTerminal(cwd || activeProject || undefined, undefined, undefined, id);
    },
    [tiles, activeProject, activateTile, addTerminal]
  );
  useRemoteTiles(openRemote);

  const handleShowWorker = ({ tileId, cwd, group }: ShowWorkerRequest) => {
    if (tiles.some((t) => t.id === tileId)) {
      activateTile(tileId);
      return;
    }
    addTerminal(cwd || undefined, undefined, undefined, tileId);
    patchTile(tileId, { runId: group.id });
  };
  handleShowWorkerRef.current = handleShowWorker;

  React.useEffect(
    () =>
      onRefDiff(({ root, path }) => {
        openDiff(root, path);
      }),
    [openDiff]
  );

  const alertKinds = React.useMemo(
    () => new Map([...alerts].map(([id, alert]): [string, NotifyKind] => [id, alert.kind])),
    [alerts]
  );

  const tileMap = React.useMemo(() => {
    const map = new Map<string, Tile>();
    for (const t of tiles) map.set(t.id, t);
    return map;
  }, [tiles]);

  const projectGroups = React.useMemo(
    () => buildProjectGroups({ frames, tiles, projects }),
    [frames, projects, tiles]
  );

  const detachedTileIds = React.useMemo(() => {
    const docked = new Set<string>();
    for (const panel of Object.values(projectView.byProject)) {
      for (const group of Object.values(panel.groups)) for (const id of group.tileIds) docked.add(id);
    }
    return new Set(tiles.filter((t) => t.type === 'term' && !docked.has(t.id)).map((t) => t.id));
  }, [projectView.byProject, tiles]);

  const projectNotes = React.useMemo(
    () => (activeProject ? tiles.filter((t) => t.type === 'note' && tileMatchesProject(t, activeProject, projects)) : []),
    [activeProject, projects, tiles]
  );

  const activeTile = activeTileId ? (tileMap.get(activeTileId) ?? null) : null;
  const activeDiff =
    activeTile?.type === 'code' && activeTile.cwd && activeTile.filePath ? fileKey(activeTile.cwd, activeTile.filePath) : null;

  const activeNote = activeTile?.type === 'note' ? activeTile : null;
  const activeNoteEditor = activeNote ? noteEditors[activeNote.id] : null;

  const requestCloseTile = React.useCallback(
    (id: string) => {
      const tile = tileMap.get(id);
      if (tile?.type === 'editor' && tile.filePath && isDirty(tile.filePath)) {
        setTileToClose({ id, path: tile.filePath });
        return;
      }
      closeTile(id);
    },
    [tileMap, closeTile]
  );

  const requestCloseTab = React.useCallback(
    (id: string) => {
      if (tileMap.get(id)?.type === 'term') {
        detachTile(id);
        return;
      }
      requestCloseTile(id);
    },
    [tileMap, detachTile, requestCloseTile]
  );
  closeTileRef.current = requestCloseTab;

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

  const handleSaveTile = () => {
    void saveAndCloseTile();
  };

  const cancelCloseTile = () => setTileToClose(null);

  const registerNoteEditor = React.useCallback((id: string, editor: EditorView | null) => {
    setNoteEditors((prev) => {
      if (editor) return { ...prev, [id]: editor };
      const next = { ...prev };
      delete next[id];
      return next;
    });
  }, []);

  const handleAgentState = React.useCallback((id: string, live: boolean, busy: boolean) => {
    setAgentStatus(id, live ? (busy ? 'busy' : 'idle') : null);
  }, []);

  const handleRenameTile = React.useCallback(
    (tileId: string, title: string) => {
      const tile = tileMap.get(tileId);
      if (!tile) return;
      if (tile.type === 'note') {
        const nextContent = applyFrontTitle(tile.content || '', title);
        patchTile(tileId, { content: nextContent, userTitle: title || undefined });
        if (activeId) void writeNote(activeId, tileId, nextContent).catch(() => {});
        return;
      }
      patchTile(tileId, { userTitle: title || undefined });
    },
    [tileMap, patchTile, activeId]
  );

  const handleDuplicateTile = React.useCallback(
    (tileId: string) => {
      const src = tileMap.get(tileId);
      const group = panelState ? Object.values(panelState.groups).find((g) => g.tileIds.includes(tileId)) : undefined;
      const newId = addTerminal(src?.cwd ?? src?.workspacePath ?? activeProject ?? undefined, group?.id);
      if (src?.userTitle) patchTile(newId, { userTitle: src.userTitle });
    },
    [tileMap, panelState, activeProject, addTerminal, patchTile]
  );

  const handleDropTab = React.useCallback(
    (opts: { tileId: string; targetGroupId: string; zone: DropZone; targetIndex?: number }) => {
      moveTile({
        tileId: opts.tileId,
        targetGroupId: opts.targetGroupId,
        zone: opts.zone,
        index: opts.targetIndex
      });
    },
    [moveTile]
  );

  const handleResizeSplit = React.useCallback(
    (path: number[], ratio: number) => {
      resizeSplit(
        path.map((step) => (step === 0 ? 'first' : 'second')),
        ratio
      );
    },
    [resizeSplit]
  );

  const addAlert = React.useCallback((id: string, kind: NotifyKind, tabId: string | null) => {
    setAlert(id, kind, tabId ?? '');
  }, []);

  const openNotified = React.useCallback(
    ({ tileId, wsId, tabId }: NotifyTarget) => {
      if (tiles.some((t) => t.id === tileId)) {
        clearAlert(tileId);
        activateTile(tileId);
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
    [tiles, tabs, activeId, activateTile, switchTab, switchWorkspace]
  );

  useNotifyBridge({
    tiles,
    wsId: activeId,
    tabId: activeTabId,
    activeTile: activeTileId,
    onOpen: openNotified,
    onAlert: addAlert,
    onClear: clearAlert
  });

  const switchToCanvasMode = React.useCallback(() => {
    if (activeTabId) void setTabMode(activeTabId, 'canvas');
  }, [activeTabId, setTabMode]);

  const runCommand = (cmd: CommandId): boolean => {
    if (cmd === 'tile.new') {
      addTerminal();
      return true;
    }
    if (cmd === 'note.new') {
      addNote();
      return true;
    }
    if (cmd === 'tile.close') {
      const id = activeTileIdRef.current;
      if (!id) return false;
      closeTileRef.current(id);
      return true;
    }
    if (cmd === 'tile.reopen') {
      reopenTile();
      return true;
    }
    if (cmd === 'tile.fullscreen') {
      toggleMaximizeGroup();
      return true;
    }
    if (cmd === 'tile.focus') {
      const id = activeTileIdRef.current;
      if (!id) return false;
      activateTile(id);
      return true;
    }
    if (cmd === 'editor.save') return dispatchSave().handled;
    if (cmd === 'editor.find') return requestFind();
    if (cmd === 'editor.preview') return requestPreview();
    if (cmd === 'view.navigator') {
      if (leftHasVisible) toggleLeftSidebar();
      return true;
    }
    if (cmd === 'view.inspector') {
      if (rightHasVisible) toggleRightSidebar();
      return true;
    }
    if (cmd === 'view.resetPanels') {
      reset();
      return true;
    }
    if (cmd === 'panel.splitRight') {
      splitGroup('horizontal', undefined, true);
      return true;
    }
    if (cmd === 'panel.splitDown') {
      splitGroup('vertical', undefined, true);
      return true;
    }
    if (cmd === 'view.palette') {
      setCommandBarOpen((v) => !v);
      return true;
    }
    if (cmd === 'view.toggleMode') {
      if (!activeTabId) return false;
      void setTabMode(activeTabId, 'canvas');
      return true;
    }
    if (cmd === 'orch.toggleMode') {
      if (activeTileId) coordinatorActions.toggle(activeTileId);
      return true;
    }
    if (cmd === 'orch.boards') {
      const tile = tiles.find((t) => t.id === activeTileId);
      const cwd = tile?.cwd ?? activeProject ?? '';
      const runId = getProjectLastBoard(cwd) ?? '';
      addOrchBoard(runId, cwd);
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
      focusFrame: () => {},
      focusTile: (id) => {
        clearAlert(id);
        activateTile(id);
      },
      runCommand: (id) => void runCommandRef.current(id),
      openWorkspace: (wsId) => void switchWorkspace(wsId)
    }
  };

  const folderPickerResolveRef = React.useRef<((p: string | null) => void) | null>(null);

  const openAddProjectModal = React.useCallback(() => setAddProjectOpen(true), []);

  const openFolderPicker = React.useCallback((): Promise<string | null> => {
    return new Promise((resolve) => {
      folderPickerResolveRef.current = resolve;
      setAddProjectOpen(true);
    });
  }, []);

  const handleSidebarActivateTile = React.useCallback((tileId: string) => activateTile(tileId), [activateTile]);

  const inspectorProject = activeProject && activeProject !== DEFAULT_PROJECT ? activeProject : (activeTile?.cwd ?? null);

  const handleInspectorNewNote = React.useCallback(() => {
    addNote(activeProject ?? undefined);
  }, [addNote, activeProject]);

  const closeCommandBar = React.useCallback(() => setCommandBarOpen(false), []);

  const handleGroupNewNote = React.useCallback(
    (groupId: string) => addNote(activeProject ?? undefined, groupId),
    [addNote, activeProject]
  );

  const handleGroupNewTerminal = React.useCallback(
    (groupId: string) => addTerminal(activeProject ?? undefined, groupId),
    [addTerminal, activeProject]
  );

  const splitTreeHandlers: PanelSplitTreeProps['handlers'] = {
    onFocusGroup: focusGroup,
    onCloseGroup: closeGroup,
    onCloseTile: requestCloseTab,
    onTerminateTile: requestCloseTile,
    onDuplicateTile: handleDuplicateTile,
    onToggleMaximize: toggleMaximizeGroup,
    onNewNote: handleGroupNewNote,
    onNewTerminal: handleGroupNewTerminal,
    onActivateTile: activateTile,
    onRenameTile: handleRenameTile,
    onPatchTile: patchTile,
    onCwd: setTileCwd,
    onOscTitle: setTileOscTitle,
    onAgentState: handleAgentState,
    onNoteEditor: registerNoteEditor,
    onOpenDiff: openDiff,
    onOpenFile: openFile,
    onOpenRunOutput: openRunOutput,
    onSplitGroup: splitGroup,
    onDragStartTab: setDraggingTab,
    onDropTab: handleDropTab,
    onResizeSplit: handleResizeSplit
  };

  const sharedGroupState: PanelSplitTreeProps['sharedState'] = {
    wsId: activeId,
    tabs,
    activeTabId,
    tileMap,
    alerts: alertKinds,
    agents,
    draggingTab
  };

  const handleAddProjectSelect = (path: string) => {
    setAddProjectOpen(false);
    const resolve = folderPickerResolveRef.current;
    folderPickerResolveRef.current = null;
    if (resolve) {
      resolve(path);
      return;
    }
    addProject(path);
  };

  const handleAddProjectClose = () => {
    setAddProjectOpen(false);
    const resolve = folderPickerResolveRef.current;
    folderPickerResolveRef.current = null;
    resolve?.(null);
  };

  const pvGroupOps = React.useMemo(
    () => ({ tiles, frames, createFrame, patchFrame, placeTiles }),
    [tiles, frames, createFrame, patchFrame, placeTiles]
  );

  const pvRenameFrame = React.useCallback(
    (id: string, title: string) => patchFrame(id, { title }),
    [patchFrame]
  );

  const pvSpawnTerminal = React.useCallback(
    (group: ProjectGroup | null) => {
      if (!group) {
        addTerminal();
        return;
      }
      if (group.kind === 'path' || group.kind === 'frame') {
        addTerminal(group.path ?? undefined);
      } else {
        addTerminal();
      }
    },
    [addTerminal]
  );

  const pvSpawnNote = React.useCallback(
    (group: ProjectGroup) => {
      addNote(group.path ?? undefined);
    },
    [addNote]
  );

  const pvSelectGroupOp = React.useCallback(
    (group: ProjectGroup) => {
      if (group.path) selectProject(group.path);
    },
    [selectProject]
  );

  const pvGetViewCenter = React.useCallback(() => ({ x: 0, y: 0 }), []);

  const projectActions = useProjectsPanelActions({
    ops: pvGroupOps,
    removeFrameOp: removeFrame,
    renameFrameOp: pvRenameFrame,
    activateTileOp: (tile) => activateTile(tile.id),
    closeTileOp: requestCloseTile,
    renameTileOp: handleRenameTile,
    spawnTerminal: pvSpawnTerminal,
    spawnNote: pvSpawnNote,
    selectGroupOp: pvSelectGroupOp,
    onAddProject: openAddProjectModal,
    openFolderPicker,
    getViewCenter: pvGetViewCenter,
    removeProjectOp: removeProject
  });

  const pvActiveGroupKey = React.useMemo(() => {
    if (!activeProject) return null;
    const norm = activeProject.toLowerCase();
    return projectGroups.find((g) => g.path?.toLowerCase() === norm)?.key ?? null;
  }, [projectGroups, activeProject]);

  const handleTopNewTerminal = () => {
    addTerminal();
  };

  const handleNoteColor = (color: string) => {
    if (activeNote) patchTile(activeNote.id, { color });
  };

  const handleLeftOpen = React.useCallback(() => setLeftOpen(true), [setLeftOpen]);
  const handleRightOpen = React.useCallback(() => setRightOpen(true), [setRightOpen]);
  const handleLeftClose = React.useCallback(() => setLeftOpen(false), [setLeftOpen]);
  const handleRightClose = React.useCallback(() => setRightOpen(false), [setRightOpen]);

  const renderPanel = React.useCallback(
    (id: PanelId): React.ReactNode => {
      switch (id) {
        case 'projects':
          return (
            <ProjectsPanel
              groups={projectGroups}
              activeTileId={activeTileId}
              activeGroupKey={pvActiveGroupKey}
              recentProjects={recentProjects}
              signals={{ alerts: alertKinds, agents, detached: detachedTileIds }}
              actions={projectActions}
            />
          );
        case 'files':
          return <FilesPanel activeProject={inspectorProject} onOpenFile={openFile} />;
        case 'git':
          return (
            <GitPanel
              activeDiff={activeDiff}
              activeProject={inspectorProject}
              onDiffFiles={setDiffFiles}
              onOpenDiff={openDiff}
              onOpenFile={openFile}
            />
          );
        case 'notes':
          return (
            <NotesPanel
              notes={projectNotes}
              activeTileId={activeTileId}
              onNewNote={handleInspectorNewNote}
              onFocusNote={handleSidebarActivateTile}
              onCloseNote={requestCloseTile}
            />
          );
        case 'docker':
          return <DockerPanel />;
        default:
          return null;
      }
    },
    [
      projectGroups,
      activeTileId,
      pvActiveGroupKey,
      recentProjects,
      alertKinds,
      agents,
      detachedTileIds,
      projectActions,
      inspectorProject,
      openFile,
      activeDiff,
      setDiffFiles,
      openDiff,
      projectNotes,
      handleInspectorNewNote,
      requestCloseTile,
      handleSidebarActivateTile
    ]
  );

  const hasActivePanel = Boolean(activeProject && panelState) && Object.keys(panelState?.groups ?? {}).length > 0;

  return (
    <div className={styles.root}>
      <div className={styles.bg}>
        <div className={styles.workspace}>
          {leftHasVisible && leftOpen && (
            <DockSidebar
              side="left"
              mode="project"
              width={leftWidth}
              panels={leftPanels}
              onWidth={setLeftWidth}
              onClose={handleLeftClose}
              renderPanel={renderPanel}
            />
          )}
          {leftHasVisible && !leftOpen && <DockRail mode="project" side="left" panels={leftPanels} onOpen={handleLeftOpen} />}

          <main className={styles.center}>
            {hasActivePanel && panelState ? (
              <PanelSplitTree
                node={panelState.layout}
                groups={panelState.groups}
                activeGroupId={panelState.activeGroupId}
                maximizedGroupId={panelState.maximizedGroupId ?? null}
                groupCount={Object.keys(panelState.groups).length}
                sharedState={sharedGroupState}
                handlers={splitTreeHandlers}
              />
            ) : (
              <div className={styles.landingWrap}>
                <div className={styles.landing}>
                  <div className={styles.landingHeader}>
                    <div className={styles.landingIcon}>
                      <FolderGit2 size={20} strokeWidth={1.75} />
                    </div>
                    <div className={styles.landingTitles}>
                      <h2 className={styles.landingTitle}>Project Panels</h2>
                      <p className={styles.landingSubtitle}>
                        Work by project folder with split-pane terminals, file editors, diffs, and notes.
                      </p>
                    </div>
                  </div>

                  <div className={styles.landingActions}>
                    <button type="button" className={styles.primaryAction} onClick={openAddProjectModal}>
                      <FolderPlus size={14} strokeWidth={2} />
                      <span>Add project folder</span>
                    </button>
                    <button type="button" className={styles.secondaryAction} onClick={handleTopNewTerminal}>
                      <SquareTerminal size={14} strokeWidth={1.75} />
                      <span>New terminal</span>
                    </button>
                    <button type="button" className={styles.secondaryAction} onClick={switchToCanvasMode}>
                      <LayoutGrid size={14} strokeWidth={1.75} />
                      <span>Switch to Canvas</span>
                    </button>
                  </div>

                  {recentProjects.length > 0 && (
                    <div className={styles.section}>
                      <span className={styles.sectionTitle}>Recent projects</span>
                      <div className={styles.recentList}>
                        {recentProjects.slice(0, 6).map((p) => {
                          const pickProject = () => addProject(p);
                          return (
                            <button key={p} type="button" onClick={pickProject} className={styles.recentItem}>
                              <span className={styles.recentMain}>
                                <FolderGit2 size={14} strokeWidth={1.75} />
                                <span className={styles.recentName}>{projectName(p)}</span>
                                <span className={styles.recentPath}>{p}</span>
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  )}

                  <div className={styles.section}>
                    <span className={styles.sectionTitle}>Shortcuts</span>
                    <div className={styles.shortcutGrid}>
                      <div className={styles.shortcutRow}>
                        <span>New terminal</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('tile.new'))}</kbd>
                      </div>
                      <div className={styles.shortcutRow}>
                        <span>Split right</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('panel.splitRight'))}</kbd>
                      </div>
                      <div className={styles.shortcutRow}>
                        <span>Split down</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('panel.splitDown'))}</kbd>
                      </div>
                      <div className={styles.shortcutRow}>
                        <span>Projects sidebar</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('view.navigator'))}</kbd>
                      </div>
                      <div className={styles.shortcutRow}>
                        <span>Right inspector</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('view.inspector'))}</kbd>
                      </div>
                      <div className={styles.shortcutRow}>
                        <span>Switch tab mode</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('view.toggleMode'))}</kbd>
                      </div>
                      <div className={styles.shortcutRow}>
                        <span>Command bar</span>
                        <kbd className={styles.shortcutKbd}>{formatCombo(getBinding('view.palette'))}</kbd>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </main>

          {rightHasVisible && rightOpen && (
            <DockSidebar
              side="right"
              mode="project"
              width={rightWidth}
              panels={rightPanels}
              onWidth={setRightWidth}
              onClose={handleRightClose}
              renderPanel={renderPanel}
            />
          )}
          {rightHasVisible && !rightOpen && (
            <DockRail mode="project" side="right" panels={rightPanels} onOpen={handleRightOpen} />
          )}
        </div>
      </div>

      <Toasts />

      {commandBarOpen && <CommandBar sources={commandBarSources} onClose={closeCommandBar} />}

      {addProjectOpen && (
        <AddProjectModal
          recentProjects={recentProjects}
          existingProjects={projects}
          initialPath={activeProject}
          onSelect={handleAddProjectSelect}
          onClose={handleAddProjectClose}
        />
      )}

      {tileToClose && (
        <UnsavedDialog paths={[tileToClose.path]} onSave={handleSaveTile} onDiscard={discardTile} onCancel={cancelCloseTile} />
      )}

      {activeNote && activeNoteEditor && (
        <div className={styles.noteToolbarDock}>
          <NoteToolbar editor={activeNoteEditor} color={activeNote.color || '#fef8c4'} onColor={handleNoteColor} />
        </div>
      )}
    </div>
  );
};

export default ProjectView;
