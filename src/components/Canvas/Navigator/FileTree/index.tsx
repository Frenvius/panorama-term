import React from 'react';
import { ChevronRight } from 'lucide-react';

import FileIcon from '~/components/commons/FileIcon';
import { readDir, createEntry, type DirEntry } from '~/adapter/fs/fs.client';

import styles from './styles.module.scss';

export interface Creating {
  dir: boolean;
  parent: string;
}

export interface FileTreeHandlers {
  onCreateEnd: (path?: string) => void;
  onOpen: (path: string, preview: boolean) => void;
  onMenu: (e: React.MouseEvent, entry: DirEntry) => void;
}

const focusInput = (el: HTMLInputElement | null) => el?.focus({ preventScroll: true });

const CHILD_PAGE = 500;

interface NodeProps {
  depth: number;
  query: string;
  entry: DirEntry;
  expanded?: boolean;
  creating: Creating | null;
  handlers: FileTreeHandlers;
}

const Node = ({ entry, depth, query, expanded, creating, handlers }: NodeProps) => {
  const { onOpen, onMenu, onCreateEnd } = handlers;
  const [open, setOpen] = React.useState(Boolean(expanded));
  const [error, setError] = React.useState<string | null>(null);
  const [children, setChildren] = React.useState<DirEntry[] | null>(null);
  const [limit, setLimit] = React.useState(CHILD_PAGE);
  const adding = creating?.parent === entry.path ? creating : null;

  React.useEffect(() => {
    if (adding) setOpen(true);
    setError(null);
  }, [adding]);

  React.useEffect(() => {
    if (!open || children) return;
    let alive = true;
    void readDir(entry.path).then((list) => {
      if (alive) setChildren(list);
    });
    return () => {
      alive = false;
    };
  }, [open, children, entry.path]);

  const toggle = () => {
    if (entry.dir) setOpen((v) => !v);
    else onOpen(entry.path, true);
  };

  const pin = () => {
    if (!entry.dir) onOpen(entry.path, false);
  };

  const menu = (e: React.MouseEvent) => onMenu(e, entry);

  const cancelCreate = () => onCreateEnd();

  const onCreateKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') return onCreateEnd();
    if (e.key !== 'Enter' || !adding) return;
    const name = e.currentTarget.value.trim();
    if (!name) return onCreateEnd();
    const sep = entry.path.includes('\\') ? '\\' : '/';
    const path = `${entry.path.replace(/[\\/]+$/, '')}${sep}${name}`;
    createEntry(path, adding.dir)
      .then(() => {
        setChildren(null);
        onCreateEnd(path);
      })
      .catch((err) => setError(String(err)));
  };

  const showMore = () => setLimit((n) => n + CHILD_PAGE);

  const hidden = query && !entry.name.toLowerCase().includes(query);
  if (hidden && !entry.dir) return null;

  const listed = query ? children?.filter((child) => child.dir || child.name.toLowerCase().includes(query)) : children;
  const shown = listed?.slice(0, limit);
  const rest = (listed?.length ?? 0) - limit;

  return (
    <>
      <div
        className={styles.row}
        style={{ paddingLeft: 8 + depth * 14 }}
        onClick={toggle}
        onDoubleClick={pin}
        onContextMenu={menu}
        data-dim={hidden || undefined}
        data-ignored={entry.ignored || undefined}
      >
        {entry.dir ? (
          <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} data-open={open || undefined} />
        ) : (
          <span className={styles.caret} />
        )}
        <FileIcon name={entry.name} dir={entry.dir} open={open} size={14} />

        <span className={styles.name}>{entry.name}</span>
      </div>
      {adding && (
        <div className={styles.row} style={{ paddingLeft: 8 + (depth + 1) * 14 }}>
          <span className={styles.caret} />
          <FileIcon name="" dir={adding.dir} size={14} />
          <input
            ref={focusInput}
            spellCheck={false}
            title={error ?? undefined}
            className={styles.input}
            onBlur={cancelCreate}
            onKeyDown={onCreateKey}
            aria-invalid={Boolean(error)}
            placeholder={adding.dir ? 'Folder name' : 'File name'}
          />
        </div>
      )}
      {open &&
        shown?.map((child) => (
          <Node key={child.path} entry={child} depth={depth + 1} query={query} creating={creating} handlers={handlers} />
        ))}
      {open && rest > 0 && (
        <div className={styles.more} style={{ paddingLeft: 24 + (depth + 1) * 14 }}>
          <span>{rest} more</span>
          <button type="button" className={styles.showMore} onClick={showMore}>
            Show more
          </button>
        </div>
      )}
    </>
  );
};

interface FileTreeProps {
  root: string;
  query: string;
  creating: Creating | null;
  handlers: FileTreeHandlers;
}

const FileTree = ({ root, query, creating, handlers }: FileTreeProps) => {
  const name = root.split(/[\\/]/).filter(Boolean).pop() ?? root;
  const entry: DirEntry = { name, path: root, dir: true };

  return <Node expanded entry={entry} depth={0} query={query} creating={creating} handlers={handlers} />;
};

export default FileTree;
