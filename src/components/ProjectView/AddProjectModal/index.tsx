import type { DirEntry } from '~/adapter/fs/fs.client';

import React from 'react';
import { X, Check, Clock, Folder, Search, ArrowUp, FolderPlus, ChevronRight } from 'lucide-react';

import styles from './styles.module.scss';
import { readDir, homeDir } from '~/adapter/fs/fs.client';
import { projectName, normalizeProjectPath } from '~/usecase/util/projectLayout';

export interface AddProjectModalProps {
  recentProjects: string[];
  existingProjects: string[];
  initialPath?: string | null;
  onSelect: (path: string) => void;
  onClose: () => void;
}

const parentDir = (raw: string): string => {
  const cleaned = raw.trim().replace(/[\\/]+$/, '');
  if (!cleaned) return 'C:\\Users';
  if (/^[A-Za-z]:$/.test(cleaned)) return `${cleaned}\\`;
  const idx = Math.max(cleaned.lastIndexOf('/'), cleaned.lastIndexOf('\\'));
  if (idx < 0) return cleaned;
  if (idx === 0) return cleaned.startsWith('/') ? '/' : cleaned;
  const head = cleaned.slice(0, idx);
  return /^[A-Za-z]:$/.test(head) ? `${head}\\` : head;
};

const resolveStartDir = (initialPath?: string | null, recentProjects: string[] = []): string => {
  if (initialPath?.trim()) return initialPath.trim();
  if (recentProjects.length > 0 && recentProjects[0]) return parentDir(recentProjects[0]);
  return '';
};

const AddProjectModal = ({ recentProjects, existingProjects, initialPath, onSelect, onClose }: AddProjectModalProps) => {
  const [inputPath, setInputPath] = React.useState(() => initialPath?.trim() ?? '');
  const [currentPath, setCurrentPath] = React.useState(() => resolveStartDir(initialPath, recentProjects));
  const [entries, setEntries] = React.useState<DirEntry[]>([]);
  const [filter, setFilter] = React.useState('');
  const [loading, setLoading] = React.useState(false);

  const existingSet = React.useMemo(
    () => new Set(existingProjects.map((p) => normalizeProjectPath(p).toLowerCase())),
    [existingProjects]
  );

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      onClose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  React.useEffect(() => {
    let alive = true;
    setLoading(true);
    if (!currentPath) {
      void homeDir().then((home) => {
        if (alive) setCurrentPath(home || '/');
      });
      return () => {
        alive = false;
      };
    }
    void readDir(currentPath).then((list) => {
      if (!alive) return;
      const dirs = list.filter((e) => e.dir);
      setEntries(dirs);
      setLoading(false);
    });
    return () => {
      alive = false;
    };
  }, [currentPath]);

  const isExisting = (path: string): boolean => existingSet.has(normalizeProjectPath(path).toLowerCase());

  const choosePath = (raw: string) => {
    const trimmed = raw.trim();
    if (!trimmed) return;
    onSelect(trimmed);
  };

  const handleDirectSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    choosePath(inputPath);
  };

  const handleBackdropMouseDown = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) onClose();
  };

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => setInputPath(e.target.value);

  const handleFilterChange = (e: React.ChangeEvent<HTMLInputElement>) => setFilter(e.target.value);

  const handleGoUp = () => {
    const next = parentDir(currentPath);
    if (next && next !== currentPath) {
      setFilter('');
      setCurrentPath(next);
    }
  };

  const handleSelectCurrent = () => choosePath(currentPath);

  const navigateInto = (dirPath: string) => {
    setFilter('');
    setCurrentPath(dirPath);
    setInputPath(dirPath);
  };

  const handleAddSubdir = (e: React.MouseEvent, dirPath: string) => {
    e.stopPropagation();
    choosePath(dirPath);
  };

  const needle = filter.trim().toLowerCase();
  const visibleDirs = React.useMemo(
    () => (needle ? entries.filter((e) => e.name.toLowerCase().includes(needle)) : entries),
    [entries, needle]
  );

  const uniqueRecents = React.useMemo(() => {
    const seen = new Set<string>();
    const out: string[] = [];
    for (const p of recentProjects) {
      const key = normalizeProjectPath(p).toLowerCase();
      if (!key || seen.has(key)) continue;
      seen.add(key);
      out.push(p);
    }
    return out;
  }, [recentProjects]);

  const canGoUp = parentDir(currentPath) !== currentPath;

  return (
    <div className={styles.backdrop} onMouseDown={handleBackdropMouseDown}>
      <div className={styles.modal} role="dialog" aria-modal="true" aria-label="Add project">
        <header className={styles.header}>
          <span className={styles.headerTitle}>
            <FolderPlus size={15} strokeWidth={2} />
            Add Project Folder
          </span>
          <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
            <X size={14} strokeWidth={2} />
          </button>
        </header>

        <div className={styles.body}>
          <form className={styles.directForm} onSubmit={handleDirectSubmit}>
            <input
              autoFocus
              value={inputPath}
              onChange={handleInputChange}
              className={styles.pathInput}
              placeholder="Enter or paste project folder path..."
            />
            <button type="submit" className={styles.primaryBtn} disabled={!inputPath.trim()}>
              <FolderPlus size={14} strokeWidth={2} />
              Add project
            </button>
          </form>

          {uniqueRecents.length > 0 && (
            <div className={styles.section}>
              <div className={styles.sectionHeader}>
                <span className={styles.sectionLabel}>
                  <Clock size={12} strokeWidth={2} />
                  Recent Projects
                </span>
              </div>
              <div className={styles.recentList}>
                {uniqueRecents.map((path) => {
                  const added = isExisting(path);
                  const pickRecent = () => choosePath(path);
                  return (
                    <button key={path} type="button" disabled={added} onClick={pickRecent} className={styles.recentRow}>
                      <span className={styles.recentMeta}>
                        <Folder size={14} strokeWidth={1.75} />
                        <span className={styles.recentName}>{projectName(path)}</span>
                        <span className={styles.recentPath}>{path}</span>
                      </span>
                      {added && (
                        <span className={styles.addedBadge}>
                          <Check size={11} strokeWidth={2.25} />
                          Added
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          <div className={styles.section}>
            <div className={styles.sectionHeader}>
              <span className={styles.sectionLabel}>
                <Folder size={12} strokeWidth={2} />
                Browse Folders
              </span>
            </div>

            <div className={styles.browserBox}>
              <div className={styles.browserBar}>
                <button type="button" onClick={handleGoUp} disabled={!canGoUp} className={styles.navBtn} title="Parent folder">
                  <ArrowUp size={13} strokeWidth={2} />
                  Up
                </button>
                <span className={styles.currentDir} title={currentPath}>
                  {currentPath}
                </span>
                <button type="button" className={styles.primaryBtn} onClick={handleSelectCurrent}>
                  Select current folder
                </button>
              </div>

              <div className={styles.filterRow}>
                <Search size={12} strokeWidth={2} />
                <input value={filter} onChange={handleFilterChange} placeholder="Filter subdirectories..." />
              </div>

              <div className={styles.dirList}>
                {visibleDirs.map((entry) => {
                  const openSubdir = () => navigateInto(entry.path);
                  const selectSubdir = () => choosePath(entry.path);
                  const addSubdir = (e: React.MouseEvent) => handleAddSubdir(e, entry.path);
                  return (
                    <div key={entry.path} onClick={openSubdir} className={styles.dirRow} onDoubleClick={selectSubdir}>
                      <span className={styles.dirNameWrap}>
                        <ChevronRight size={13} strokeWidth={2} />
                        <Folder size={14} strokeWidth={1.75} />
                        <span className={styles.dirName}>{entry.name}</span>
                      </span>
                      <button type="button" className={styles.addDirBtn} onClick={addSubdir}>
                        Add
                      </button>
                    </div>
                  );
                })}
                {!loading && visibleDirs.length === 0 && (
                  <div className={styles.emptyDirs}>
                    {needle ? 'No matching subdirectories' : 'No subdirectories in this folder'}
                  </div>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default AddProjectModal;
