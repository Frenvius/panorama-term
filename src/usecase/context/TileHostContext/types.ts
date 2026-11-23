import type { EditorView } from '@codemirror/view';

import React from 'react';

import type { FsBox } from '~/domain/interfaces/canvas.interface';
import type { RunSummary } from '~/domain/interfaces/orch.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { TabMeta } from '~/domain/interfaces/workspace.interface';
import type { BoardProject } from '~/usecase/util/boardProject';

export interface TileStatus {
  live: boolean;
  active: boolean;
  hidden: boolean;
  visible: boolean;
  selected: boolean;
  fullscreen: boolean;
  alert: NotifyKind | null;
}

export interface TileLink {
  active: boolean;
  target: { id: string; name: string } | null;
  linkedTerms: { id: string; name: string }[];
}

export interface BoardActions {
  allRuns: RunSummary[];
  projectOf: (tileId: string) => BoardProject;
  runsForTile: (tileId: string) => RunSummary[];
  openBoard: (runId: string, nearTileId?: string, repo?: string, frameId?: string) => void;
  newBoard: (repo: string, nearTileId?: string, frameId?: string) => void;
  createProject: (tileId: string) => void;
  setFolder: (frameId: string) => void;
}

export interface TileActions {
  onSnap: (id: string) => void;
  onClose: (id: string) => void;
  onReveal: (id: string) => void;
  onActivate: (id: string) => void;
  onCopyNote: (id: string) => void;
  onCopyPath: (id: string) => void;
  onPasteNote: (id: string) => void;
  onToggleRaw: (id: string) => void;
  onTogglePin: (id: string) => void;
  onDuplicate: (id: string) => void;
  onToggleSelect: (id: string) => void;
  onToggleFullscreen: (id: string) => void;
  onCopyNoteSelection: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onOscTitle: (id: string, title: string) => void;
  onNoteTitle: (id: string, title: string) => void;
  onLink: (noteId: string, termId: string) => void;
  onPatchOrchTile: (id: string, patch: { runId?: string; boardTitle?: string }) => void;
  onNoteChange: (id: string, content: string) => void;
  unlinkTerm: (noteId: string, termId: string) => void;
  unlinkNote: (noteId: string, termId: string) => void;
  onMove: (id: string, dx: number, dy: number) => void;
  onFocusTile: (id: string, zoomToMax?: boolean) => void;
  onMoveToTab: (id: string, targetTabId: string) => void;
  onCwd: (id: string, cwd: string, branch?: string) => void;
  onNoteEditor: (id: string, editor: EditorView | null) => void;
  onAgentState: (id: string, live: boolean, busy: boolean) => void;
  onLinkDragStart: (noteId: string, e: React.PointerEvent) => void;
  onResize: (id: string, dir: string, dx: number, dy: number) => void;
  onOpenRunOutput: (srcId: string, cwd: string, sessionId: string, cmd: string) => void;
}

export interface TileHostValue {
  fsBox: FsBox;
  tabs: TabMeta[];
  exiting: boolean;
  wsId: string | null;
  actions: TileActions;
  boards: BoardActions;
  activeTabId: string | null;
}

export interface TileHostProviderProps {
  value: TileHostValue;
  children: React.ReactNode;
}
