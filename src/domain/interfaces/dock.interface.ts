export type DockSide = 'left' | 'right';

export type DockMode = 'canvas' | 'project';

export type PanelId = 'projects' | 'notes' | 'files' | 'git' | 'docker';

export interface DockLayout {
  left: PanelId[];
  right: PanelId[];
  hidden: PanelId[];
  active: { left: PanelId | null; right: PanelId | null };
}
