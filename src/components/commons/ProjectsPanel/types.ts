import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { RunSummary } from '~/domain/interfaces/orch.interface';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';
import type { DropPlace } from '~/usecase/util/projectOrder';
import type { TidyState } from '~/usecase/hooks/useTidyAI';

export interface ProjectsPanelSignals {
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
  dormant?: Set<string>;
  detached?: Set<string>;
}

export interface ProjectsPanelActions {
  onActivateTile: (tile: Tile) => void;
  onCloseTile: (id: string) => void;
  onRenameTile: (id: string, title: string) => void;
  onSelectGroup: (group: ProjectGroup) => void;
  onNewTerminal: (group: ProjectGroup | null) => void;
  onNewNote: (group: ProjectGroup) => void;
  onStopTerminals: (group: ProjectGroup) => void;
  onAddProject: () => void;
  onNewFrame: () => void;
  onFixFrame: (group: ProjectGroup) => void;
  onMoveTileIntoFrame: (tile: Tile, frameId: string) => void;
  onMoveTileToGroup: (tile: Tile, group: ProjectGroup) => void;
  onRenameFrame: (frameId: string, title: string) => void;
  onDeleteFrame: (frameId: string) => void;
  onSetFolder: (frameId: string) => void;
  onRemoveProject?: (path: string) => void;
  onReorderProject: (path: string, targetPath: string, place: DropPlace) => void;
  onReorderFrame: (frameId: string, targetFrameId: string, place: DropPlace) => void;
  onOpenBoard?: (runId: string, group: ProjectGroup) => void;
  onNewBoard?: (group: ProjectGroup) => void;
  allRuns?: RunSummary[];
  orphanRuns?: RunSummary[];
}

export interface ProjectsPanelProps {
  groups: ProjectGroup[];
  activeTileId: string | null;
  activeGroupKey: string | null;
  recentProjects: string[];
  signals: ProjectsPanelSignals;
  actions: ProjectsPanelActions;
  tidy?: TidyState;
}

export type { TidyState };
