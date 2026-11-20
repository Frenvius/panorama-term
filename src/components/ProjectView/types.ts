import type { EditorView } from '@codemirror/view';

import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { Tile, TabMeta } from '~/domain/interfaces/workspace.interface';
import type {
  DropZone,
  PanelGroup,
  InspectorTab,
  SplitDirection,
  PanelLayoutNode
} from '~/domain/interfaces/projectView.interface';

export interface DraggingTabState {
  tileId: string;
  sourceGroupId: string;
}

export interface ProjectSidebarState {
  width: number;
  projects: string[];
  recentProjects: string[];
  activeTileId: string | null;
  activeProject: string | null;
  tilesByProject: Map<string, Tile[]>;
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
}

export interface ProjectSidebarHandlers {
  onClose: () => void;
  onAddProject: () => void;
  onWidth: (width: number) => void;
  onSelectProject: (path: string) => void;
  onRemoveProject: (path: string) => void;
  onNewNote: (projectPath?: string) => void;
  onCloseTile: (tileId: string) => void;
  onActivateTile: (tileId: string) => void;
  onNewTerminal: (projectPath?: string) => void;
  onRenameTile: (tileId: string, title: string) => void;
}

export interface InspectorSidebarState {
  width: number;
  notes: Tile[];
  tab: InspectorTab;
  activeDiff: string | null;
  activeTileId: string | null;
  activeProject: string | null;
}

export interface InspectorSidebarHandlers {
  onClose: () => void;
  onNewNote: () => void;
  onWidth: (width: number) => void;
  onCommandBar: () => void;
  onTabChange: (tab: InspectorTab) => void;
  onFocusNote: (tileId: string) => void;
  onCloseNote: (tileId: string) => void;
  onDiffFiles: (files: string[]) => void;
  onOpenFile: (root: string, filePath: string, opts?: { preview?: boolean; sideSplit?: boolean }) => void;
  onOpenDiff: (root: string, filePath: string, opts?: { sideSplit?: boolean }) => void;
}

export interface PanelGroupViewState {
  wsId: string | null;
  group: PanelGroup;
  isFocused: boolean;
  isMaximized: boolean;
  canCloseGroup: boolean;
  tabs: TabMeta[];
  activeTabId: string | null;
  tileMap: Map<string, Tile>;
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
  draggingTab: DraggingTabState | null;
}

export interface PanelGroupViewHandlers {
  onFocusGroup: (groupId: string) => void;
  onCloseGroup: (groupId: string) => void;
  onCloseTile: (tileId: string) => void;
  onTerminateTile: (tileId: string) => void;
  onDuplicateTile: (tileId: string) => void;
  onToggleMaximize: (groupId: string) => void;
  onNewNote: (groupId: string) => void;
  onNewTerminal: (groupId: string) => void;
  onActivateTile: (tileId: string, groupId: string) => void;
  onRenameTile: (tileId: string, title: string) => void;
  onPatchTile: (tileId: string, patch: Partial<Tile>) => void;
  onCwd: (tileId: string, cwd: string, branch?: string) => void;
  onOscTitle: (tileId: string, title: string) => void;
  onAgentState: (tileId: string, live: boolean, busy: boolean) => void;
  onNoteEditor: (tileId: string, editor: EditorView | null) => void;
  onOpenDiff: (root: string, filePath: string, opts?: { sideSplit?: boolean }) => void;
  onOpenFile: (root: string, filePath: string, opts?: { preview?: boolean; sideSplit?: boolean }) => void;
  onOpenRunOutput: (srcId: string, cwd: string, sessionId: string, cmd: string) => void;
  onSplitGroup: (direction: SplitDirection, groupId: string, spawnTerm?: boolean) => void;
  onDragStartTab: (drag: DraggingTabState | null) => void;
  onDropTab: (opts: { tileId: string; targetGroupId: string; zone: DropZone; targetIndex?: number }) => void;
}

export interface PanelSplitTreeProps {
  node: PanelLayoutNode;
  groups: Record<string, PanelGroup>;
  activeGroupId: string;
  maximizedGroupId: string | null;
  groupCount: number;
  sharedState: Omit<PanelGroupViewState, 'group' | 'isFocused' | 'isMaximized' | 'canCloseGroup'>;
  handlers: PanelGroupViewHandlers & {
    onResizeSplit: (path: number[], ratio: number) => void;
    onGridSlack: (px: number) => void;
  };
}
