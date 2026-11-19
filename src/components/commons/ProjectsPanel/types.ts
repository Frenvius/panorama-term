import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { NotifyKind } from '~/domain/interfaces/notify.interface';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';

export interface ProjectsPanelSignals {
  alerts: Map<string, NotifyKind>;
  agents: Map<string, 'idle' | 'busy'>;
}

export interface ProjectsPanelActions {
  onActivateTile: (tile: Tile) => void;
  onCloseTile: (id: string) => void;
  onRenameTile: (id: string, title: string) => void;
  onSelectGroup: (group: ProjectGroup) => void;
  onNewTerminal: (group: ProjectGroup | null) => void;
  onNewNote: (group: ProjectGroup) => void;
  onAddProject: () => void;
  onNewFrame: () => void;
  onFixFrame: (group: ProjectGroup) => void;
  onMoveTileIntoFrame: (tile: Tile, frameId: string) => void;
  onRenameFrame: (frameId: string, title: string) => void;
  onDeleteFrame: (frameId: string) => void;
  onSetFolder: (frameId: string) => void;
  onRemoveProject?: (path: string) => void;
}

export interface ProjectsPanelProps {
  groups: ProjectGroup[];
  activeTileId: string | null;
  activeGroupKey: string | null;
  recentProjects: string[];
  signals: ProjectsPanelSignals;
  actions: ProjectsPanelActions;
}
