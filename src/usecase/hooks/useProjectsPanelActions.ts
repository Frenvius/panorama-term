import type { Frame, Tile } from '~/domain/interfaces/workspace.interface';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';
import type { GroupOps } from '~/usecase/util/projectGroupActions';
import type { ProjectsPanelActions } from '~/components/commons/ProjectsPanel/types';

import React from 'react';

import { findFreeRect } from '~/usecase/util/frameLayout';
import { sleepIdleTerminals } from '~/usecase/util/dormant';
import { currentLayoutSpacing } from '~/usecase/util/reorderSetting';
import { fixGroupFrame, moveTileToGroup, moveTileIntoFrame } from '~/usecase/util/projectGroupActions';
import type { DropPlace } from '~/usecase/util/projectOrder';
import { projectName, normalizeProjectPath } from '~/usecase/util/projectLayout';
import { FRAME_COLOR, FRAME_WIDTH, FRAME_HEIGHT } from '~/usecase/util/constants';

export interface UseProjectsPanelActionsOpts {
  ops: GroupOps;
  removeFrameOp: (id: string) => void;
  renameFrameOp: (id: string, title: string) => void;
  activateTileOp: (tile: Tile) => void;
  closeTileOp: (id: string) => void;
  renameTileOp: (id: string, title: string) => void;
  spawnTerminal: (group: ProjectGroup | null) => void;
  spawnNote: (group: ProjectGroup) => void;
  selectGroupOp: (group: ProjectGroup) => void;
  onAddProject: () => void;
  openFolderPicker: () => Promise<string | null>;
  getViewCenter: () => { x: number; y: number };
  removeProjectOp?: (path: string) => void;
  reorderProjectOp: (from: string, to: string, place: DropPlace) => void;
  reorderFrameOp: (from: string, to: string, place: DropPlace) => void;
  sleepTileOp: (id: string) => void;
  patchTileOp: (id: string, patch: Partial<Tile>) => void;
  onTileMovedOp?: (id: string) => void;
}

export const useProjectsPanelActions = (opts: UseProjectsPanelActionsOpts): ProjectsPanelActions => {
  const ref = React.useRef(opts);
  ref.current = opts;

  const onActivateTile = React.useCallback((tile: Tile) => ref.current.activateTileOp(tile), []);

  const onCloseTile = React.useCallback((id: string) => ref.current.closeTileOp(id), []);

  const onRenameTile = React.useCallback((id: string, title: string) => ref.current.renameTileOp(id, title), []);

  const onSelectGroup = React.useCallback((group: ProjectGroup) => ref.current.selectGroupOp(group), []);

  const onNewTerminal = React.useCallback((group: ProjectGroup | null) => ref.current.spawnTerminal(group), []);

  const onNewNote = React.useCallback((group: ProjectGroup) => ref.current.spawnNote(group), []);

  const onAddProject = React.useCallback(() => ref.current.onAddProject(), []);

  const onNewFrame = React.useCallback(() => {
    const { ops, getViewCenter } = ref.current;
    const near = getViewCenter();
    const obstacles = [
      ...ops.tiles.map((t) => ({ x: t.x, y: t.y, width: t.width, height: t.height })),
      ...ops.frames.map((f) => ({ x: f.x, y: f.y, width: f.width, height: f.height }))
    ];
    const gap = currentLayoutSpacing().gap;
    const { x, y } = findFreeRect({ gap, near, obstacles, width: FRAME_WIDTH, height: FRAME_HEIGHT });
    ops.createFrame({ x, y, width: FRAME_WIDTH, height: FRAME_HEIGHT, title: 'Frame', color: FRAME_COLOR });
  }, []);

  const onFixFrame = React.useCallback((group: ProjectGroup) => {
    const { ops, getViewCenter } = ref.current;
    const near = getViewCenter();
    fixGroupFrame({ ops, group, near });
  }, []);

  const onMoveTileIntoFrame = React.useCallback((tile: Tile, frameId: string) => {
    moveTileIntoFrame({ ops: ref.current.ops, tile, frameId });
  }, []);

  const onMoveTileToGroup = React.useCallback((tile: Tile, group: ProjectGroup) => {
    const { ops, patchTileOp, onTileMovedOp } = ref.current;
    moveTileToGroup({ ops, tile, group, patchTile: patchTileOp });
    onTileMovedOp?.(tile.id);
  }, []);

  const onRenameFrame = React.useCallback((frameId: string, title: string) => {
    ref.current.renameFrameOp(frameId, title);
  }, []);

  const onDeleteFrame = React.useCallback((frameId: string) => {
    ref.current.removeFrameOp(frameId);
  }, []);

  const onSetFolder = React.useCallback((frameId: string) => {
    void (async () => {
      const path = await ref.current.openFolderPicker();
      if (!path) return;
      const norm = normalizeProjectPath(path);
      const frame = ref.current.ops.frames.find((f) => f.id === frameId);
      const patch: Partial<Omit<Frame, 'id'>> = { path: norm };
      if (frame?.title === 'Frame') patch.title = projectName(norm);
      ref.current.ops.patchFrame(frameId, patch);
    })();
  }, []);

  const onRemoveProject = React.useCallback((path: string) => {
    ref.current.removeProjectOp?.(path);
  }, []);

  const hasRemoveProject = Boolean(opts.removeProjectOp);

  const onReorderProject = React.useCallback((path: string, targetPath: string, place: DropPlace) => {
    ref.current.reorderProjectOp(path, targetPath, place);
  }, []);

  const onReorderFrame = React.useCallback((frameId: string, targetFrameId: string, place: DropPlace) => {
    ref.current.reorderFrameOp(frameId, targetFrameId, place);
  }, []);

  const onStopTerminals = React.useCallback((group: ProjectGroup) => {
    void sleepIdleTerminals(group.tiles, ref.current.sleepTileOp);
  }, []);

  return React.useMemo(
    () => ({
      onStopTerminals,
      onActivateTile,
      onCloseTile,
      onRenameTile,
      onSelectGroup,
      onNewTerminal,
      onNewNote,
      onAddProject,
      onNewFrame,
      onFixFrame,
      onMoveTileIntoFrame,
      onMoveTileToGroup,
      onRenameFrame,
      onDeleteFrame,
      onSetFolder,
      onRemoveProject: hasRemoveProject ? onRemoveProject : undefined,
      onReorderProject,
      onReorderFrame
    }),
    [
      hasRemoveProject,
      onActivateTile,
      onAddProject,
      onCloseTile,
      onDeleteFrame,
      onFixFrame,
      onMoveTileIntoFrame,
      onMoveTileToGroup,
      onNewFrame,
      onNewNote,
      onNewTerminal,
      onRemoveProject,
      onRenameFrame,
      onRenameTile,
      onReorderFrame,
      onReorderProject,
      onSelectGroup,
      onSetFolder,
      onStopTerminals
    ]
  );
};
