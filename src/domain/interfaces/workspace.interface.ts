export type TileType = 'term' | 'note' | 'code' | 'image' | 'graph' | 'browser';

export interface Tile {
  id: string;
  type: TileType;
  x: number;
  y: number;
  width: number;
  height: number;
  zIndex: number;
  cwd?: string;
  branch?: string;
  url?: string | null;
  filePath?: string;
  autoTitle?: string;
  oscTitle?: string;
  userTitle?: string;
  folderPath?: string;
  workspacePath?: string;
  ptySessionId?: string;
  color?: string;
  content?: string;
  pinned?: boolean;
  renderOnly?: boolean;
  linkedTo?: string[];
  runCwd?: string;
}

export interface Frame {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  title: string;
  color: string;
}

export interface Viewport {
  zoom: number;
  centerX: number;
  centerY: number;
}

export interface CanvasState {
  version: 1;
  tiles: Tile[];
  viewport: Viewport;
  frames?: Frame[];
}

export interface TabState {
  id: string;
  name: string;
  state: CanvasState;
}

export interface TabMeta {
  id: string;
  name: string;
}

export interface WorkspaceMeta {
  id: string;
  name: string;
  color: string;
  createdAt: number;
  lastFocusedAt: number;
}

export interface WorkspaceFile {
  meta: WorkspaceMeta;
  tabs: TabState[];
  activeTabId: string;
}

export interface WorkspaceIndex {
  version: 1;
  activeId: string | null;
  order: string[];
}
