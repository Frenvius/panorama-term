import React from 'react';
import {
  Eye,
  Copy,
  Check,
  History,
  ArrowUp,
  ListTree,
  GitBranch,
  RefreshCw,
  FolderOpen,
  ChevronDown,
  CircleCheck,
  ChevronRight,
  ListCollapse,
  LoaderCircle,
  GitCompareArrows
} from 'lucide-react';

import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { FileChange, StatusSnapshot, CommitMessageEntry } from '~/domain/interfaces/git.interface';
import FileIcon from '~/components/commons/FileIcon';
import ContextMenu from '~/components/commons/ContextMenu';
import { revealPath } from '~/adapter/shell/shell.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import {
  gitStatus,
  gitCommit,
  gitAddIgnore,
  gitPushCurrent,
  gitLogMessages,
  gitUnpushedCommits
} from '~/adapter/git/git.client';

import styles from './styles.module.scss';

interface GitTabProps {
  root: string;
  query: string;
  active: string | null;
  onFiles: (files: string[]) => void;
  onOpenDiff: (file: string) => void;
}

const stopClick = (e: React.MouseEvent) => e.stopPropagation();

const STATUS_COLOR: Record<string, string> = {
  modified: '#5781ea',
  added: '#6fb14e',
  deleted: '#c75450',
  renamed: '#c796e7',
  copied: '#c796e7',
  untracked: '#bb956c',
  conflicted: '#ff6262'
};

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const statusKey = (file: FileChange): string => {
  if (file.is_untracked) return 'untracked';
  const x = file.status_index;
  const y = file.status_worktree;
  if (x === 'U' || y === 'U') return 'conflicted';
  if (x === 'A' || y === 'A') return 'added';
  if (x === 'D' || y === 'D') return 'deleted';
  if (x === 'R') return 'renamed';
  if (x === 'C') return 'copied';
  return 'modified';
};

const displayDir = (dir: string): string => dir.replace(/\//g, '\\');

const pluralize = (n: number): string => (n === 1 ? '1 file' : `${n} files`);

type TreeNode =
  | { kind: 'folder'; id: string; name: string; children: TreeNode[] }
  | { kind: 'file'; id: string; change: FileChange };

const buildDirTree = (files: FileChange[], prefix: string): TreeNode[] => {
  const root: TreeNode[] = [];
  for (const file of files) {
    const segments = file.dir ? file.dir.split('/').filter(Boolean) : [];
    let current = root;
    let sofar = '';
    for (const segment of segments) {
      sofar = sofar ? `${sofar}/${segment}` : segment;
      let folder = current.find(
        (n): n is Extract<TreeNode, { kind: 'folder' }> => n.kind === 'folder' && n.name === segment
      );
      if (!folder) {
        folder = { kind: 'folder', id: `${prefix}:${sofar}`, name: segment, children: [] };
        current.push(folder);
      }
      current = folder.children;
    }
    current.push({ kind: 'file', id: `${prefix}:${file.path}`, change: file });
  }
  return root;
};

const collectPaths = (nodes: TreeNode[]): string[] =>
  nodes.flatMap((node) => (node.kind === 'file' ? [node.change.path] : collectPaths(node.children)));

const collectFolderIds = (nodes: TreeNode[]): string[] =>
  nodes.flatMap((node) => (node.kind === 'folder' ? [node.id, ...collectFolderIds(node.children)] : []));

const flattenTree = (nodes: TreeNode[], shut: (id: string) => boolean): string[] =>
  nodes.flatMap((node) => {
    if (node.kind === 'file') return [node.change.path];
    return shut(node.id) ? [] : flattenTree(node.children, shut);
  });

interface TriCheckboxProps {
  state: 'all' | 'none' | 'partial';
  onChange: (on: boolean) => void;
}

const TriCheckbox = ({ state, onChange }: TriCheckboxProps) => {
  const ref = React.useRef<HTMLInputElement>(null);

  React.useEffect(() => {
    if (ref.current) ref.current.indeterminate = state === 'partial';
  }, [state]);

  const change = (e: React.ChangeEvent<HTMLInputElement>) => onChange(e.target.checked);
  const stop = (e: React.MouseEvent) => e.stopPropagation();

  return <input ref={ref} type="checkbox" checked={state === 'all'} onChange={change} onClick={stop} />;
};

const GitTab = ({ root, query, active, onFiles, onOpenDiff }: GitTabProps) => {
  const listRef = React.useRef<HTMLDivElement>(null);
  const [status, setStatus] = React.useState<StatusSnapshot | null>(null);
  const [selected, setSelected] = React.useState<Set<string>>(() => new Set());
  const [msg, setMsg] = React.useState('');
  const [amend, setAmend] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [unpushed, setUnpushed] = React.useState(0);
  const [pushing, setPushing] = React.useState(false);
  const [refreshing, setRefreshing] = React.useState(false);
  const [history, setHistory] = React.useState<CommitMessageEntry[] | null>(null);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(() => new Set());
  const [groupBy, setGroupBy] = React.useState<'directory' | 'module'>('module');
  const [amendMenu, setAmendMenu] = React.useState<CommitMessageEntry[] | null>(null);
  const [viewMenu, setViewMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [fileMenu, setFileMenu] = React.useState<{ x: number; y: number; rel: string; file: FileChange | null } | null>(
    null
  );
  const lastCommit = React.useRef<CommitMessageEntry | null>(null);
  const known = React.useRef<Set<string>>(new Set());
  const commitRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!history && !amendMenu) return;
    const outside = (e: PointerEvent) => {
      if (commitRef.current?.contains(e.target as Node)) return;
      setHistory(null);
      setAmendMenu(null);
    };
    document.addEventListener('pointerdown', outside, true);
    return () => document.removeEventListener('pointerdown', outside, true);
  }, [history, amendMenu]);

  const applySelection = React.useCallback((snap: StatusSnapshot) => {
    const changed = new Set(snap.changes.map((f) => f.path));
    const all = [...snap.changes, ...snap.unversioned].map((f) => f.path);
    setSelected((prev) => {
      const next = new Set<string>();
      for (const path of all) {
        if (known.current.has(path)) {
          if (prev.has(path)) next.add(path);
        } else if (changed.has(path)) {
          next.add(path);
        }
      }
      return next;
    });
    known.current = new Set(all);
  }, []);

  const fetchStatus = React.useCallback(
    (quiet: boolean) => {
      if (!quiet) setRefreshing(true);
      Promise.all([gitStatus(root), gitUnpushedCommits(root).catch(() => [])])
        .then(([snap, ahead]) => {
          setStatus(snap);
          setUnpushed(ahead.length);
          setError(null);
          applySelection(snap);
        })
        .catch((err: unknown) => {
          if (!quiet) setError(message(err));
        })
        .finally(() => {
          if (!quiet) setRefreshing(false);
        });
      gitLogMessages(root, 1)
        .then((entries) => (lastCommit.current = entries[0] ?? null))
        .catch(() => (lastCommit.current = null));
    },
    [root, applySelection]
  );

  const load = React.useCallback(() => fetchStatus(false), [fetchStatus]);

  React.useEffect(load, [load]);

  React.useEffect(() => {
    const timer = window.setInterval(() => {
      if (!busy && !pushing) fetchStatus(true);
    }, 5000);
    return () => window.clearInterval(timer);
  }, [fetchStatus, busy, pushing]);

  const needle = query.trim().toLowerCase();
  const matches = (file: FileChange): boolean => !needle || file.path.toLowerCase().includes(needle);

  const visible = React.useMemo(() => {
    if (!status) return [];

    const shut = (id: string) => !needle && collapsed.has(id);
    const sections: Array<[string, FileChange[]]> = [
      ['changes', status.changes],
      ['unversioned', status.unversioned]
    ];
    const out: string[] = [];

    for (const [id, files] of sections) {
      if (files.length === 0 || shut(id)) continue;
      const shown = files.filter((file) => !needle || file.path.toLowerCase().includes(needle));
      if (groupBy === 'directory') out.push(...flattenTree(buildDirTree(shown, id), shut));
      else out.push(...shown.map((file) => file.path));
    }

    return out;
  }, [status, needle, collapsed, groupBy]);

  React.useEffect(() => {
    onFiles(visible);
  }, [visible, onFiles]);

  React.useEffect(() => {
    if (!active) return;
    listRef.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const openDiff = (path: string) => {
    listRef.current?.focus({ preventScroll: true });
    onOpenDiff(path);
  };

  const onArrows = (e: React.KeyboardEvent) => {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (visible.length === 0) return;
    e.preventDefault();

    const step = e.key === 'ArrowDown' ? 1 : -1;
    const at = active ? visible.indexOf(active) : -1;
    const next = at === -1 ? (step === 1 ? 0 : visible.length - 1) : at + step;
    if (next < 0 || next >= visible.length) return;

    onOpenDiff(visible[next]);
  };

  const toggle = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const setMany = (paths: string[], on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const path of paths) {
        if (on) next.add(path);
        else next.delete(path);
      }
      return next;
    });
  };

  const triState = (paths: string[]): 'all' | 'none' | 'partial' => {
    const on = paths.filter((p) => selected.has(p)).length;
    if (on === 0) return 'none';
    return on === paths.length ? 'all' : 'partial';
  };

  const toggleCollapse = (id: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const expandAll = () => setCollapsed(new Set());

  const collapseAll = () => {
    if (!status) return;
    const ids = ['changes', 'unversioned'];
    ids.push(...collectFolderIds(buildDirTree(status.changes, 'changes')));
    ids.push(...collectFolderIds(buildDirTree(status.unversioned, 'unversioned')));
    setCollapsed(new Set(ids));
  };

  const openViewMenu = (e: React.MouseEvent) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setViewMenu({ x: rect.left, y: rect.bottom + 4 });
  };
  const closeViewMenu = () => setViewMenu(null);

  const groupDirectory = () => setGroupBy('directory');
  const groupModule = () => setGroupBy('module');

  const openFileMenu = (e: React.MouseEvent, file: FileChange) => {
    e.preventDefault();
    e.stopPropagation();
    setFileMenu({ x: e.clientX, y: e.clientY, rel: file.path, file });
  };

  const openFolderMenu = (e: React.MouseEvent, rel: string) => {
    e.preventDefault();
    e.stopPropagation();
    setFileMenu({ x: e.clientX, y: e.clientY, rel, file: null });
  };

  const closeFileMenu = () => setFileMenu(null);

  const sep = root.includes('/') ? '/' : '\\';
  const absPath = (rel: string) => root + sep + rel.replace(/\//g, sep);
  const absDir = (file: FileChange) => (file.dir ? root + sep + file.dir.replace(/\//g, sep) : root);

  const addIgnore = (rel: string, local: boolean) => {
    gitAddIgnore(root, rel, local)
      .then(() => fetchStatus(true))
      .catch((err: unknown) => setError(message(err)));
  };

  const gitEntry = (rel: string): ContextMenuEntry => ({
    label: 'Git',
    icon: <GitBranch size={15} strokeWidth={1.75} />,
    submenu: [
      { label: 'Add local exclude', onSelect: () => addIgnore(rel, true) },
      { label: 'Add to .gitignore', onSelect: () => addIgnore(rel, false) }
    ]
  });

  const menuItems = (rel: string, file: FileChange | null): ContextMenuEntry[] => {
    if (!file) return [gitEntry(rel)];
    return [
      {
        label: 'Commit file',
        icon: <Check size={15} strokeWidth={1.75} />,
        onSelect: () => setMany([file.path], true)
      },
      {
        label: 'Show diff',
        icon: <GitCompareArrows size={15} strokeWidth={1.75} />,
        onSelect: () => openDiff(file.path)
      },
      'separator',
      { label: 'Copy path', icon: <Copy size={15} strokeWidth={1.75} />, onSelect: () => writeClipboard(absPath(rel)) },
      { label: 'Copy relative path', icon: <span />, onSelect: () => writeClipboard(rel) },
      'separator',
      {
        label: 'Show in Explorer',
        icon: <FolderOpen size={15} strokeWidth={1.75} />,
        onSelect: () => revealPath(absDir(file))
      },
      'separator',
      gitEntry(rel)
    ];
  };

  const viewItems: ContextMenuEntry[] = [
    { label: 'Directory', icon: groupBy === 'directory' ? <Check size={15} strokeWidth={2} /> : <span />, onSelect: groupDirectory },
    { label: 'Module', icon: groupBy === 'module' ? <Check size={15} strokeWidth={2} /> : <span />, onSelect: groupModule }
  ];

  const toggleAmend = (e: React.ChangeEvent<HTMLInputElement>) => {
    const on = e.target.checked;
    setAmend(on);
    const last = lastCommit.current;
    if (on && last && msg.trim() === '') setMsg(last.body);
    else if (!on && last && msg === last.body) setMsg('');
  };

  const commit = (push: boolean) => {
    setBusy(true);
    setError(null);
    gitCommit(root, [...selected], msg, amend)
      .then(() => (push ? gitPushCurrent(root).then(() => undefined) : undefined))
      .then(() => {
        setMsg('');
        setAmend(false);
        load();
      })
      .catch((err: unknown) => setError(message(err)))
      .finally(() => setBusy(false));
  };

  const doCommit = () => commit(false);
  const doCommitPush = () => commit(true);

  const push = () => {
    setPushing(true);
    setError(null);
    gitPushCurrent(root)
      .then(load)
      .catch((err: unknown) => setError(message(err)))
      .finally(() => setPushing(false));
  };

  const openHistory = () => {
    if (history) {
      setHistory(null);
      return;
    }
    setAmendMenu(null);
    void gitLogMessages(root, 20)
      .then(setHistory)
      .catch(() => setHistory([]));
  };

  const pickHistory = (entry: CommitMessageEntry) => {
    setMsg(entry.body);
    setHistory(null);
  };

  const openAmendMenu = () => {
    if (amendMenu) {
      setAmendMenu(null);
      return;
    }
    setHistory(null);
    void gitUnpushedCommits(root)
      .then(setAmendMenu)
      .catch(() => setAmendMenu([]));
  };

  const pickAmend = (entry: CommitMessageEntry) => {
    setMsg(entry.body);
    setAmendMenu(null);
  };

  const onMsg = (e: React.ChangeEvent<HTMLTextAreaElement>) => setMsg(e.target.value);

  const canCommit = !busy && selected.size > 0 && (msg.trim().length > 0 || amend);

  const renderNode = (node: TreeNode, depth: number): React.ReactNode => {
    const pad = 8 + depth * 14;

    if (node.kind === 'folder') {
      const shut = !needle && collapsed.has(node.id);
      const paths = collectPaths(node.children);
      const open = () => toggleCollapse(node.id);
      const check = (on: boolean) => setMany(paths, on);
      const menu = (e: React.MouseEvent) => openFolderMenu(e, node.id.slice(node.id.indexOf(':') + 1));

      return (
        <div key={node.id}>
          <div className={styles.row} style={{ paddingLeft: pad }} onClick={open} onContextMenu={menu}>
            {shut ? (
              <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} />
            ) : (
              <ChevronDown size={12} strokeWidth={2.5} className={styles.caret} />
            )}
            <TriCheckbox state={triState(paths)} onChange={check} />
            <FileIcon dir open={!shut} size={14} />
            <span className={styles.name}>{node.name}</span>
          </div>
          {!shut && node.children.map((child) => renderNode(child, depth + 1))}
        </div>
      );
    }

    const file = node.change;
    const key = statusKey(file);
    const pick = () => toggle(file.path);
    const view = () => openDiff(file.path);
    const menu = (e: React.MouseEvent) => openFileMenu(e, file);

    return (
      <div
        key={node.id}
        className={styles.row}
        style={{ paddingLeft: pad + 16 }}
        title={file.path}
        onClick={view}
        onContextMenu={menu}
        data-active={file.path === active}
      >
        <input type="checkbox" checked={selected.has(file.path)} onChange={pick} onClick={stopClick} />
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.name}
          style={{ color: STATUS_COLOR[key], textDecoration: key === 'deleted' ? 'line-through' : undefined }}
        >
          {file.name}
        </span>
      </div>
    );
  };

  const flatRow = (file: FileChange) => {
    const key = statusKey(file);
    const pick = () => toggle(file.path);
    const view = () => openDiff(file.path);
    const menu = (e: React.MouseEvent) => openFileMenu(e, file);
    return (
      <div
        key={file.path}
        className={styles.row}
        style={{ paddingLeft: 43 }}
        title={file.path}
        onClick={view}
        onContextMenu={menu}
        data-active={file.path === active}
      >
        <input type="checkbox" checked={selected.has(file.path)} onChange={pick} onClick={stopClick} />
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.fileName}
          style={{ color: STATUS_COLOR[key], textDecoration: key === 'deleted' ? 'line-through' : undefined }}
        >
          {file.name}
        </span>
        {file.dir && <span className={styles.dir}>{displayDir(file.dir)}</span>}
      </div>
    );
  };

  const section = (id: string, label: string, files: FileChange[]) => {
    if (files.length === 0) return null;
    const shown = files.filter(matches);
    const tree = groupBy === 'directory' ? buildDirTree(shown, id) : null;
    const paths = shown.map((f) => f.path);
    const shut = !needle && collapsed.has(id);
    const open = () => toggleCollapse(id);
    const check = (on: boolean) => setMany(paths, on);

    return (
      <div>
        <div className={styles.sectionHead} onClick={open}>
          {shut ? (
            <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} />
          ) : (
            <ChevronDown size={12} strokeWidth={2.5} className={styles.caret} />
          )}
          <TriCheckbox state={triState(paths)} onChange={check} />
          <span className={styles.sectionLabel}>{label}</span>
          <span className={styles.count}>{pluralize(files.length)}</span>
        </div>
        {!shut && (tree ? tree.map((node) => renderNode(node, 1)) : shown.map(flatRow))}
      </div>
    );
  };

  const clean = status && status.changes.length === 0 && status.unversioned.length === 0;

  const blocked = Boolean(error && !status);
  const friendly = error && error.includes('not a git repository') ? 'Not a git repository' : error;

  return (
    <div className={styles.root}>
      <div className={styles.toolbar}>
        <button className={styles.tool} onClick={load} disabled={refreshing} title="Refresh" aria-label="Refresh">
          <RefreshCw size={12} strokeWidth={2} className={refreshing ? styles.spinning : undefined} />
        </button>
        <button className={styles.tool} onClick={openViewMenu} disabled={blocked} title="Group by" aria-label="Group by">
          <Eye size={12} strokeWidth={2} />
        </button>
        <button className={styles.tool} onClick={expandAll} disabled={blocked} title="Expand all" aria-label="Expand all">
          <ListTree size={12} strokeWidth={2} />
        </button>
        <button
          className={styles.tool}
          onClick={collapseAll}
          disabled={blocked}
          title="Collapse all"
          aria-label="Collapse all"
        >
          <ListCollapse size={12} strokeWidth={2} />
        </button>
      </div>

      <div ref={listRef} className={styles.list} tabIndex={-1} onKeyDown={onArrows}>
        {!status && !error && (
          <div className={styles.notice}>
            <LoaderCircle size={16} strokeWidth={2} className={styles.spinning} />
          </div>
        )}
        {blocked && <div className={styles.notice}>{friendly}</div>}
        {clean && (
          <div className={styles.notice}>
            <CircleCheck size={16} strokeWidth={1.75} />
            Working tree clean
          </div>
        )}
        {status && section('changes', 'Changes', status.changes)}
        {status && section('unversioned', 'Unversioned', status.unversioned)}
      </div>

      <div className={styles.commit} ref={commitRef}>
        <div className={styles.commitBar}>
          <label className={styles.amend}>
            <input type="checkbox" checked={amend} onChange={toggleAmend} disabled={blocked} />
            Amend
          </label>
          <button className={styles.amendPick} onClick={openAmendMenu} disabled={blocked}>
            last commit
            <ChevronDown size={11} strokeWidth={2} />
          </button>
          <span className={styles.spacer} />
          <button
            className={styles.tool}
            onClick={openHistory}
            disabled={blocked}
            title="Recent messages"
            aria-label="Recent messages"
          >
            <History size={12} strokeWidth={2} />
          </button>
        </div>

        {amendMenu && (
          <div className={styles.history}>
            {amendMenu.length === 0 && <div className={styles.hint}>No commits</div>}
            {amendMenu.map((entry) => {
              const pick = () => pickAmend(entry);
              return (
                <button key={entry.short} className={styles.historyRow} onClick={pick}>
                  <span className={styles.historySubject}>{entry.subject}</span>
                  <span className={styles.historyMeta}>
                    {entry.short} - {entry.date}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        {history && (
          <div className={styles.history}>
            {history.length === 0 && <div className={styles.hint}>No commits yet</div>}
            {history.map((entry) => {
              const pick = () => pickHistory(entry);
              return (
                <button key={entry.short} className={styles.historyRow} onClick={pick}>
                  <span className={styles.historySubject}>{entry.subject}</span>
                  <span className={styles.historyMeta}>
                    {entry.short} - {entry.date}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        <textarea
          className={styles.message}
          value={msg}
          onChange={onMsg}
          disabled={blocked}
          placeholder="Commit message"
          spellCheck={false}
        />

        {error && !blocked && <div className={styles.error}>{error}</div>}

        <div className={styles.buttons}>
          <button className={styles.primary} onClick={doCommit} disabled={!canCommit}>
            {busy ? 'Working...' : 'Commit'}
          </button>
          {clean && unpushed > 0 ? (
            <button className={styles.secondary} onClick={push} disabled={pushing}>
              <ArrowUp size={12} strokeWidth={2} />
              {pushing ? 'Pushing...' : `Push ${unpushed} ${unpushed === 1 ? 'commit' : 'commits'}`}
            </button>
          ) : (
            <button className={styles.secondary} onClick={doCommitPush} disabled={!canCommit}>
              Commit and Push...
            </button>
          )}
        </div>
      </div>

      {viewMenu && <ContextMenu x={viewMenu.x} y={viewMenu.y} items={viewItems} onClose={closeViewMenu} />}
      {fileMenu && (
        <ContextMenu x={fileMenu.x} y={fileMenu.y} items={menuItems(fileMenu.rel, fileMenu.file)} onClose={closeFileMenu} />
      )}
    </div>
  );
};

export default GitTab;
