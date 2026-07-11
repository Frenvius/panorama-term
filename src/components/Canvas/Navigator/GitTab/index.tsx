import React from 'react';
import {
  Eye,
  Check,
  History,
  ArrowUp,
  ListTree,
  RefreshCw,
  ChevronDown,
  CircleCheck,
  ChevronRight,
  ListCollapse
} from 'lucide-react';

import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { FileChange, StatusSnapshot, CommitMessageEntry } from '~/domain/interfaces/git.interface';
import FileIcon from '~/components/commons/FileIcon';
import ContextMenu from '~/components/commons/ContextMenu';
import {
  gitStatus,
  gitCommit,
  gitPushCurrent,
  gitLogMessages,
  gitUnpushedCommits
} from '~/adapter/git/git.client';

import styles from './styles.module.scss';

interface GitTabProps {
  root: string;
  query: string;
}

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

const GitTab = ({ root, query }: GitTabProps) => {
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
  const lastCommit = React.useRef<CommitMessageEntry | null>(null);

  const load = React.useCallback(() => {
    setRefreshing(true);
    Promise.all([gitStatus(root), gitUnpushedCommits(root).catch(() => [])])
      .then(([snap, ahead]) => {
        setStatus(snap);
        setUnpushed(ahead.length);
        setError(null);
        setSelected(new Set(snap.changes.map((f) => f.path)));
      })
      .catch((err: unknown) => setError(message(err)))
      .finally(() => setRefreshing(false));
    gitLogMessages(root, 1)
      .then((entries) => (lastCommit.current = entries[0] ?? null))
      .catch(() => (lastCommit.current = null));
  }, [root]);

  React.useEffect(load, [load]);

  const needle = query.trim().toLowerCase();
  const matches = (file: FileChange): boolean => !needle || file.path.toLowerCase().includes(needle);

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

      return (
        <div key={node.id}>
          <div className={styles.row} style={{ paddingLeft: pad }} onClick={open}>
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

    return (
      <label key={node.id} className={styles.row} style={{ paddingLeft: pad + 16 }} title={file.path}>
        <input type="checkbox" checked={selected.has(file.path)} onChange={pick} />
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.name}
          style={{ color: STATUS_COLOR[key], textDecoration: key === 'deleted' ? 'line-through' : undefined }}
        >
          {file.name}
        </span>
      </label>
    );
  };

  const flatRow = (file: FileChange) => {
    const key = statusKey(file);
    const pick = () => toggle(file.path);
    return (
      <label key={file.path} className={styles.row} style={{ paddingLeft: 43 }} title={file.path}>
        <input type="checkbox" checked={selected.has(file.path)} onChange={pick} />
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.fileName}
          style={{ color: STATUS_COLOR[key], textDecoration: key === 'deleted' ? 'line-through' : undefined }}
        >
          {file.name}
        </span>
        {file.dir && <span className={styles.dir}>{displayDir(file.dir)}</span>}
      </label>
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

      <div className={styles.list}>
        {!status && !error && <div className={styles.hint}>Loading...</div>}
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

      <div className={styles.commit}>
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
    </div>
  );
};

export default GitTab;
