import type { PanelId } from '~/domain/interfaces/dock.interface';
import type { LucideProps } from 'lucide-react';
import type { ComponentType } from 'react';

import { Container, GitBranch, FolderGit2, FolderTree, StickyNote } from 'lucide-react';

export interface PanelDescriptor {
  id: PanelId;
  label: string;
  icon: ComponentType<LucideProps>;
}

export const PANEL_DESCRIPTORS: Record<PanelId, PanelDescriptor> = {
  projects: { id: 'projects', label: 'Projects', icon: FolderGit2 },
  files: { id: 'files', label: 'Files', icon: FolderTree },
  git: { id: 'git', label: 'Source Control', icon: GitBranch },
  notes: { id: 'notes', label: 'Project Notes', icon: StickyNote },
  docker: { id: 'docker', label: 'Docker', icon: Container }
};
