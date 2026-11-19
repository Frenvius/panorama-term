export type TabMode = 'canvas' | 'project';

export type SplitDirection = 'horizontal' | 'vertical';

export type InspectorTab = 'files' | 'git' | 'docker' | 'notes';

export type DropZone = 'left' | 'right' | 'top' | 'bottom' | 'center';

export interface PanelGroup {
  id: string;
  tileIds: string[];
  activeTileId: string | null;
}

export interface PanelLeafNode {
  type: 'leaf';
  groupId: string;
}

export interface PanelSplitNode {
  ratio: number;
  type: 'split';
  first: PanelLayoutNode;
  second: PanelLayoutNode;
  direction: SplitDirection;
}

export type PanelLayoutNode = PanelLeafNode | PanelSplitNode;

export interface ProjectPanelState {
  layout: PanelLayoutNode;
  activeGroupId: string;
  maximizedGroupId?: string | null;
  groups: Record<string, PanelGroup>;
}

export interface ProjectViewState {
  leftOpen?: boolean;
  rightOpen?: boolean;
  leftWidth?: number;
  rightWidth?: number;
  projects: string[];
  activeProject: string | null;
  byProject: Record<string, ProjectPanelState>;
}
