import React from 'react';
import { X, Eye, Check, ListTree, ChevronRight, ListCollapse, LoaderCircle } from 'lucide-react';

import type { TreeNode } from '~/usecase/util/fileTree';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { LogRow, FileChange } from '~/domain/interfaces/git.interface';
import FileIcon from '~/components/commons/FileIcon';
import ContextMenu from '~/components/commons/ContextMenu';
import { gitCommitFiles } from '~/adapter/git/git.client';
import { sortTree, statusKey, STATUS_COLOR, buildDirTree, collectFolderIds } from '~/usecase/util/fileTree';

import styles from './styles.module.scss';

interface FilesProps {
  root: string;
  commit: LogRow;
  active: string | null;
  exiting?: boolean;
  onOpenDiff: (file: string, commit: string) => void;
  onClose: () => void;
}

const GROUP_KEY = 'panorama:gitCommitGroup';
const HEIGHT_KEY = 'panorama:gitCommitHeight';
const MIN_HEIGHT = 96;
const MAX_HEIGHT = 720;

const savedHeight = (): number => {
  const raw = Number(localStorage.getItem(HEIGHT_KEY));
  return raw >= MIN_HEIGHT && raw <= MAX_HEIGHT ? raw : 240;
};

const savedGroup = (): boolean => localStorage.getItem(GROUP_KEY) === 'directory';

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const subject = (row: LogRow): string => row.message.split('\n', 1)[0];

const displayDir = (dir: string): string => dir.replace(/\//g, '\\');

const label = (file: FileChange): string => (file.rename_from ? `${file.name} (from ${file.rename_from})` : file.name);

const Files = ({ root, commit, active, exiting, onOpenDiff, onClose }: FilesProps) => {
  const [files, setFiles] = React.useState<FileChange[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [byDir, setByDir] = React.useState(savedGroup);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(() => new Set());
  const [viewMenu, setViewMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [height, setHeight] = React.useState(savedHeight);

  React.useEffect(() => {
    let alive = true;
    setFiles(null);
    setError(null);
    gitCommitFiles(root, commit.short)
      .then((next) => alive && setFiles(next))
      .catch((err: unknown) => alive && setError(message(err)));
    return () => {
      alive = false;
    };
  }, [root, commit.short]);

  React.useEffect(() => {
    localStorage.setItem(GROUP_KEY, byDir ? 'directory' : 'list');
  }, [byDir]);

  const tree = React.useMemo(
    () => (files && byDir ? sortTree(buildDirTree(files, commit.short)) : null),
    [files, byDir, commit.short]
  );

  const startResize = (e: React.PointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    const startY = e.clientY;
    const startH = height;
    let next = startH;
    el.setPointerCapture(e.pointerId);

    const move = (ev: PointerEvent) => {
      next = Math.min(MAX_HEIGHT, Math.max(MIN_HEIGHT, startH + startY - ev.clientY));
      setHeight(next);
    };
    const up = () => {
      el.removeEventListener('pointermove', move);
      el.removeEventListener('pointerup', up);
      localStorage.setItem(HEIGHT_KEY, String(next));
    };
    el.addEventListener('pointermove', move);
    el.addEventListener('pointerup', up);
  };

  const groupDirectory = () => setByDir(true);
  const groupModule = () => setByDir(false);

  const openViewMenu = (e: React.MouseEvent) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setViewMenu({ x: rect.left, y: rect.bottom + 4 });
  };

  const closeViewMenu = () => setViewMenu(null);

  const viewItems: ContextMenuEntry[] = [
    { label: 'Directory', icon: byDir ? <Check size={15} strokeWidth={2} /> : <span />, onSelect: groupDirectory },
    { label: 'Module', icon: byDir ? <span /> : <Check size={15} strokeWidth={2} />, onSelect: groupModule }
  ];

  const toggleFolder = (id: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const expandAll = () => setCollapsed(new Set());

  const collapseAll = () => setCollapsed(new Set(tree ? collectFolderIds(tree) : []));

  const fileRow = (file: FileChange, pad: number, dir: boolean) => {
    const key = statusKey(file);
    const open = () => onOpenDiff(file.path, commit.short);
    return (
      <div
        key={file.path}
        className={styles.row}
        style={{ paddingLeft: pad }}
        title={file.path}
        onClick={open}
        data-active={file.path === active || undefined}
      >
        {dir && <span className={styles.caret} />}
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.name}
          style={{ color: STATUS_COLOR[key], textDecoration: key === 'deleted' ? 'line-through' : undefined }}
        >
          {label(file)}
        </span>
        {!dir && file.dir && <span className={styles.dir}>{displayDir(file.dir)}</span>}
      </div>
    );
  };

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => {
    const pad = 8 + depth * 14;
    if (node.kind === 'file') return fileRow(node.change, pad, true);

    const shut = collapsed.has(node.id);
    const open = () => toggleFolder(node.id);

    return (
      <div key={node.id}>
        <div className={styles.row} style={{ paddingLeft: pad }} onClick={open}>
          <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} data-open={!shut || undefined} />
          <FileIcon dir open={!shut} size={14} />
          <span className={styles.name}>{node.name}</span>
        </div>
        {!shut && node.children.map((child) => renderNode(child, depth + 1))}
      </div>
    );
  };

  const body = () => {
    if (error) return <div className={styles.notice}>{error}</div>;
    if (!files)
      return (
        <div className={styles.notice}>
          <LoaderCircle size={16} strokeWidth={2} className={styles.spinning} />
        </div>
      );
    if (files.length === 0) return <div className={styles.notice}>No files changed</div>;
    if (tree) return tree.map((node) => renderNode(node, 0));
    return files.map((file) => fileRow(file, 8, false));
  };

  return (
    <div className={styles.root} style={{ height }} data-exit={exiting || undefined}>
      <div className={styles.resizer} onPointerDown={startResize} />
      <div className={styles.head}>
        <span className={styles.hash}>{commit.short}</span>
        <span className={styles.subject} title={commit.message}>
          {subject(commit)}
        </span>
        <button className={styles.tool} onClick={openViewMenu} title="Group by" aria-label="Group by">
          <Eye size={12} strokeWidth={2} />
        </button>
        <button className={styles.tool} onClick={expandAll} title="Expand all" aria-label="Expand all">
          <ListTree size={12} strokeWidth={2} />
        </button>
        <button className={styles.tool} onClick={collapseAll} title="Collapse all" aria-label="Collapse all">
          <ListCollapse size={12} strokeWidth={2} />
        </button>
        <button className={styles.tool} onClick={onClose} title="Close" aria-label="Close">
          <X size={13} strokeWidth={2} />
        </button>
      </div>
      <div className={styles.list}>{body()}</div>

      {viewMenu && <ContextMenu x={viewMenu.x} y={viewMenu.y} items={viewItems} onClose={closeViewMenu} />}
    </div>
  );
};

export default Files;
