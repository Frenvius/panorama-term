import type { DirEntry } from '~/adapter/fs/fs.client';
import type { Creating } from '~/components/Canvas/Navigator/FileTree';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import React from 'react';
import { Search, Pencil, Columns2, FilePlus, FolderOpen, FolderPlus, ClipboardCopy } from 'lucide-react';

import { revealPath } from '~/adapter/shell/shell.client';
import ContextMenu from '~/components/commons/ContextMenu';
import FileTree from '~/components/Canvas/Navigator/FileTree';
import CrashNotice from '~/components/commons/CrashNotice';
import ErrorBoundary from '~/components/commons/ErrorBoundary';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import styles from '~/components/ProjectView/panels.module.scss';

export interface FilesPanelProps {
  activeProject: string | null;
  onOpenFile: (root: string, filePath: string, opts?: { preview?: boolean; sideSplit?: boolean }) => void;
}

interface FileMenuState {
  x: number;
  y: number;
  entry: DirEntry;
}

const crashFallback = (retry: () => void) => (
  <CrashNotice title="Files panel hit an error" action="Reload panel" onAction={retry} />
);

const FilesPanel = ({ activeProject, onOpenFile }: FilesPanelProps) => {
  const [query, setQuery] = React.useState('');
  const [menu, setMenu] = React.useState<FileMenuState | null>(null);
  const [creating, setCreating] = React.useState<Creating | null>(null);

  const needle = query.trim().toLowerCase();

  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);

  const handleOpenFile = (filePath: string, preview = false) => {
    if (activeProject) onOpenFile(activeProject, filePath, { preview });
  };

  const handleFileContextMenu = (e: React.MouseEvent, entry: DirEntry) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, entry });
  };

  const handleBlankContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    if (!activeProject || e.target !== e.currentTarget) return;
    setMenu({ x: e.clientX, y: e.clientY, entry: { name: '', path: activeProject, dir: true } });
  };

  const startCreate = (entry: DirEntry, dir: boolean) => {
    const parent = entry.dir ? entry.path : entry.path.replace(/[\\/][^\\/]+$/, '');
    setCreating({ parent, dir });
  };

  const endCreate = (createdPath?: string) => {
    if (createdPath && !creating?.dir) handleOpenFile(createdPath, false);
    setCreating(null);
  };

  const closeMenu = () => setMenu(null);

  const fileTreeHandlers = {
    onOpen: handleOpenFile,
    onMenu: handleFileContextMenu,
    onCreateEnd: endCreate
  };

  const buildFileMenuItems = (entry: DirEntry): ContextMenuEntry[] => [
    {
      label: 'New file',
      icon: <FilePlus size={15} strokeWidth={1.75} />,
      onSelect: () => startCreate(entry, false)
    },
    {
      label: 'New folder',
      icon: <FolderPlus size={15} strokeWidth={1.75} />,
      onSelect: () => startCreate(entry, true)
    },
    {
      label: 'Open',
      icon: <Pencil size={15} strokeWidth={1.75} />,
      disabled: entry.dir,
      onSelect: () => handleOpenFile(entry.path, false)
    },
    {
      label: 'Open in split',
      icon: <Columns2 size={15} strokeWidth={1.75} />,
      disabled: entry.dir || !activeProject,
      onSelect: () => {
        if (activeProject) onOpenFile(activeProject, entry.path, { sideSplit: true });
      }
    },
    {
      label: 'Reveal in explorer',
      icon: <FolderOpen size={15} strokeWidth={1.75} />,
      onSelect: () => revealPath(entry.path)
    },
    {
      label: 'Copy path',
      icon: <ClipboardCopy size={15} strokeWidth={1.75} />,
      onSelect: () => writeClipboard(entry.path)
    }
  ];

  return (
    <>
      <div className={styles.searchBar}>
        <div className={styles.filter}>
          <Search size={12} strokeWidth={2} />
          <input value={query} onChange={handleQueryChange} placeholder="Filter" />
        </div>
      </div>

      <div className={styles.body} onContextMenu={handleBlankContextMenu}>
        {activeProject ? (
          <ErrorBoundary fallback={crashFallback}>
            <FileTree query={needle} creating={creating} key={activeProject} root={activeProject} handlers={fileTreeHandlers} />
          </ErrorBoundary>
        ) : (
          <div className={styles.empty}>Select a project to browse files</div>
        )}
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} onClose={closeMenu} items={buildFileMenuItems(menu.entry)} />}
    </>
  );
};

export default FilesPanel;
