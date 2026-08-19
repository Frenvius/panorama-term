import React from 'react';
import { ChevronRight } from 'lucide-react';

import FileIcon from '~/components/commons/FileIcon';
import { readDir, type DirEntry } from '~/adapter/fs/fs.client';

import styles from './styles.module.scss';

interface NodeProps {
  entry: DirEntry;
  depth: number;
  query: string;
  expanded?: boolean;
  onOpen: (path: string, preview: boolean) => void;
  onMenu: (e: React.MouseEvent, entry: DirEntry) => void;
}

const Node = ({ entry, depth, query, expanded, onOpen, onMenu }: NodeProps) => {
  const [open, setOpen] = React.useState(Boolean(expanded));
  const [children, setChildren] = React.useState<DirEntry[] | null>(null);

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

  const hidden = query && !entry.name.toLowerCase().includes(query);
  if (hidden && !entry.dir) return null;

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
      {open &&
        children?.map((child) => (
          <Node key={child.path} entry={child} depth={depth + 1} query={query} onOpen={onOpen} onMenu={onMenu} />
        ))}
    </>
  );
};

interface FileTreeProps {
  root: string;
  query: string;
  onOpen: (path: string, preview: boolean) => void;
  onMenu: (e: React.MouseEvent, entry: DirEntry) => void;
}

const FileTree = ({ root, query, onOpen, onMenu }: FileTreeProps) => {
  const name = root.split(/[\\/]/).filter(Boolean).pop() ?? root;
  const entry: DirEntry = { name, path: root, dir: true };

  return <Node expanded entry={entry} depth={0} query={query} onOpen={onOpen} onMenu={onMenu} />;
};

export default FileTree;
