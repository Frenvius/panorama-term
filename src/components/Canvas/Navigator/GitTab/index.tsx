import React from 'react';
import {
  Eye,
  Copy,
  Check,
  Undo2,
  History,
  ArrowUp,
  PenLine,
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

import type { TreeNode } from '~/usecase/util/fileTree';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { RepoEntry, FileChange, StatusSnapshot, CommitMessageEntry } from '~/domain/interfaces/git.interface';
import Dialog from '~/components/commons/Dialog';
import FileIcon from '~/components/commons/FileIcon';
import ContextMenu from '~/components/commons/ContextMenu';
import Log from '~/components/Canvas/Navigator/GitTab/History';
import { revealPath } from '~/adapter/shell/shell.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import {
  splitKey,
  changeKey,
  statusKey,
  STATUS_COLOR,
  buildDirTree,
  collectKeys,
  collectFiles,
  flattenTree,
  collectFolderIds
} from '~/usecase/util/fileTree';
import {
  gitRepos,
  gitStatus,
  gitCommit,
  gitAddIgnore,
  gitPushCurrent,
  gitRollbackFile,
  gitLogMessages,
  gitUnpushedCommits
} from '~/adapter/git/git.client';
import { matchCombo, getBinding, formatCombo } from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

interface GitTabProps {
  root: string;
  query: string;
  active: string | null;
  onFiles: (files: string[]) => void;
  onOpenDiff: (repo: string, file: string, commit?: string) => void;
  onOpenFile: (file: string) => void;
}

const stopClick = (e: React.MouseEvent) => e.stopPropagation();

const VIEW_KEY = 'panorama:gitView';
const VIEWS = ['changes', 'history'] as const;
type View = (typeof VIEWS)[number];

const SECTIONS = [
  ['changes', 'Changes'],
  ['unversioned', 'Unversioned']
] as const;

const savedView = (): View => {
  const raw = localStorage.getItem(VIEW_KEY);
  return VIEWS.includes(raw as View) ? (raw as View) : 'changes';
};

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const displayDir = (dir: string): string => dir.replace(/\//g, '\\');

const pluralize = (n: number): string => (n === 1 ? '1 file' : `${n} files`);

const repoKey = (section: string, repo: string): string => `${section}::${repo}`;

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

const rollbackText = (files: FileChange[]): string => {
  if (files.length === 1) {
    const file = files[0];
    return file.is_untracked || statusKey(file) === 'added'
      ? `'${file.path}' is not in the last commit and will be deleted.`
      : `Revert changes in '${file.path}' to the last commit?`;
  }
  const gone = files.filter((f) => f.is_untracked || statusKey(f) === 'added').length;
  const base = `Revert changes in ${pluralize(files.length)} to the last commit?`;
  return gone > 0 ? `${base} ${pluralize(gone)} not in the last commit will be deleted.` : base;
};

const GitTab = ({ root, query, active, onFiles, onOpenDiff, onOpenFile }: GitTabProps) => {
  const listRef = React.useRef<HTMLDivElement>(null);
  const [view, setView] = React.useState<View>(savedView);
  const [repos, setRepos] = React.useState<RepoEntry[]>([]);
  const [statuses, setStatuses] = React.useState<Record<string, StatusSnapshot>>({});
  const [selected, setSelected] = React.useState<Set<string>>(() => new Set());
  const [msg, setMsg] = React.useState('');
  const [amend, setAmend] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [unpushed, setUnpushed] = React.useState<Record<string, number>>({});
  const [pushing, setPushing] = React.useState(false);
  const [refreshing, setRefreshing] = React.useState(false);
  const [history, setHistory] = React.useState<CommitMessageEntry[] | null>(null);
  const [collapsed, setCollapsed] = React.useState<Set<string>>(() => new Set());
  const [groupBy, setGroupBy] = React.useState<'directory' | 'module'>('module');
  const [amendMenu, setAmendMenu] = React.useState<CommitMessageEntry[] | null>(null);
  const [viewMenu, setViewMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [fileMenu, setFileMenu] = React.useState<{
    x: number;
    y: number;
    repo: string;
    rel: string;
    file: FileChange | null;
    files: FileChange[];
  } | null>(null);
  const [sectionMenu, setSectionMenu] = React.useState<{
    x: number;
    y: number;
    id: string;
    keys: string[];
    files: FileChange[];
  } | null>(null);
  const [rollback, setRollback] = React.useState<FileChange[] | null>(null);
  const [rollbackBusy, setRollbackBusy] = React.useState(false);
  const lastCommit = React.useRef<CommitMessageEntry | null>(null);
  const known = React.useRef<Set<string>>(new Set());
  const commitRef = React.useRef<HTMLDivElement>(null);

  const primary = repos[0]?.root ?? root;

  React.useEffect(() => {
    localStorage.setItem(VIEW_KEY, view);
  }, [view]);

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

  const applySelection = React.useCallback((snaps: Record<string, StatusSnapshot>) => {
    const changed = new Set<string>();
    const all: string[] = [];
    for (const snap of Object.values(snaps)) {
      for (const file of snap.changes) {
        changed.add(changeKey(file));
        all.push(changeKey(file));
      }
      for (const file of snap.unversioned) all.push(changeKey(file));
    }
    setSelected((prev) => {
      const next = new Set<string>();
      for (const key of all) {
        if (known.current.has(key)) {
          if (prev.has(key)) next.add(key);
        } else if (changed.has(key)) {
          next.add(key);
        }
      }
      return next;
    });
    known.current = new Set(all);
  }, []);

  React.useEffect(() => {
    setRepos([]);
    setStatuses({});
    gitRepos(root)
      .then(setRepos)
      .catch((err: unknown) => setError(message(err)));
  }, [root]);

  const fetchStatus = React.useCallback(
    (quiet: boolean) => {
      if (repos.length === 0) return;
      if (!quiet) setRefreshing(true);
      Promise.all(
        repos.map((repo) =>
          Promise.all([gitStatus(repo.root), gitUnpushedCommits(repo.root).catch(() => [])]).then(
            ([snap, ahead]) => [repo.root, snap, ahead.length] as const
          )
        )
      )
        .then((rows) => {
          const snaps: Record<string, StatusSnapshot> = {};
          const ahead: Record<string, number> = {};
          for (const [key, snap, count] of rows) {
            snaps[key] = snap;
            ahead[key] = count;
          }
          setStatuses(snaps);
          setUnpushed(ahead);
          setError(null);
          applySelection(snaps);
        })
        .catch((err: unknown) => {
          if (!quiet) setError(message(err));
        })
        .finally(() => {
          if (!quiet) setRefreshing(false);
        });
      gitLogMessages(primary, 1)
        .then((entries) => (lastCommit.current = entries[0] ?? null))
        .catch(() => (lastCommit.current = null));
    },
    [repos, primary, applySelection]
  );

  const load = React.useCallback(() => fetchStatus(false), [fetchStatus]);

  React.useEffect(load, [load]);

  React.useEffect(() => {
    const timer = window.setInterval(() => {
      if (!busy && !pushing && view === 'changes') fetchStatus(true);
    }, 5000);
    return () => window.clearInterval(timer);
  }, [fetchStatus, busy, pushing, view]);

  const needle = query.trim().toLowerCase();
  const matches = (file: FileChange): boolean => !needle || file.path.toLowerCase().includes(needle);

  const nestedPrefixes = React.useCallback(
    (repo: string): string[] =>
      repos.filter((r) => r.root !== repo && r.root.startsWith(`${repo}/`)).map((r) => `${r.root.slice(repo.length + 1)}/`),
    [repos]
  );

  const filesOf = React.useCallback(
    (repo: string, id: string): FileChange[] => {
      const snap = statuses[repo];
      if (!snap) return [];
      if (id === 'changes') return snap.changes;
      const nested = nestedPrefixes(repo);
      return snap.unversioned.filter((file) => !nested.some((prefix) => `${file.path}/`.startsWith(prefix)));
    },
    [statuses, nestedPrefixes]
  );

  const multi =
    repos.filter((repo) => SECTIONS.some(([id]) => filesOf(repo.root, id).length > 0)).length > 1;

  const sectionFiles = React.useCallback(
    (id: string): FileChange[] => repos.flatMap((repo) => filesOf(repo.root, id)),
    [repos, filesOf]
  );

  const visible = React.useMemo(() => {
    const shut = (id: string) => !needle && collapsed.has(id);
    const out: string[] = [];

    for (const [id] of SECTIONS) {
      if (shut(id)) continue;
      for (const repo of repos) {
        const shown = filesOf(repo.root, id).filter((file) => !needle || file.path.toLowerCase().includes(needle));
        if (shown.length === 0) continue;
        const rid = repoKey(id, repo.root);
        if (multi && shut(rid)) continue;
        if (groupBy === 'directory') out.push(...flattenTree(buildDirTree(shown, rid), shut));
        else out.push(...shown.map(changeKey));
      }
    }

    return out;
  }, [repos, filesOf, needle, collapsed, groupBy, multi]);

  React.useEffect(() => {
    onFiles(visible);
  }, [visible, onFiles]);

  React.useEffect(() => {
    if (!active) return;
    listRef.current?.querySelector('[data-active="true"]')?.scrollIntoView({ block: 'nearest' });
  }, [active]);

  const openDiff = (key: string) => {
    listRef.current?.focus({ preventScroll: true });
    const { repo, path } = splitKey(key);
    onOpenDiff(repo, path);
  };

  const openCommitDiff = (file: string, commit?: string) => onOpenDiff(primary, file, commit);

  const sepOf = (repo: string) => (repo.includes('/') ? '/' : '\\');
  const absPath = (repo: string, rel: string) => repo + sepOf(repo) + rel.replace(/\//g, sepOf(repo));
  const absDir = (file: FileChange) => (file.dir ? absPath(file.repo, file.dir) : file.repo);

  const onListKeys = (e: React.KeyboardEvent) => {
    if (matchCombo(e.nativeEvent, getBinding('diff.editFile'))) {
      if (!active) return;
      e.preventDefault();
      const { repo, path } = splitKey(active);
      onOpenFile(absPath(repo, path));
      return;
    }
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    if (visible.length === 0) return;
    e.preventDefault();

    const step = e.key === 'ArrowDown' ? 1 : -1;
    const at = active ? visible.indexOf(active) : -1;
    const next = at === -1 ? (step === 1 ? 0 : visible.length - 1) : at + step;
    if (next < 0 || next >= visible.length) return;

    openDiff(visible[next]);
  };

  const toggle = (key: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const setMany = (keys: string[], on: boolean) => {
    setSelected((prev) => {
      const next = new Set(prev);
      for (const key of keys) {
        if (on) next.add(key);
        else next.delete(key);
      }
      return next;
    });
  };

  const triState = (keys: string[]): 'all' | 'none' | 'partial' => {
    const on = keys.filter((k) => selected.has(k)).length;
    if (on === 0) return 'none';
    return on === keys.length ? 'all' : 'partial';
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

  const sectionIds = React.useCallback(
    (id: string): string[] => {
      const ids = [id];
      for (const repo of repos) {
        const rid = repoKey(id, repo.root);
        ids.push(rid, ...collectFolderIds(buildDirTree(filesOf(repo.root, id), rid)));
      }
      return ids;
    },
    [repos, filesOf]
  );

  const collapseAll = () => setCollapsed(new Set(SECTIONS.flatMap(([id]) => sectionIds(id))));

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
    setFileMenu({ x: e.clientX, y: e.clientY, repo: file.repo, rel: file.path, file, files: [file] });
  };

  const openFolderMenu = (e: React.MouseEvent, repo: string, rel: string, files: FileChange[]) => {
    e.preventDefault();
    e.stopPropagation();
    setFileMenu({ x: e.clientX, y: e.clientY, repo, rel, file: null, files });
  };

  const closeFileMenu = () => setFileMenu(null);

  const closeSectionMenu = () => setSectionMenu(null);

  const collapseSection = (id: string) => setCollapsed((prev) => new Set([...prev, ...sectionIds(id)]));

  const expandSection = (id: string) => {
    const ids = new Set(sectionIds(id));
    setCollapsed((prev) => new Set([...prev].filter((x) => !ids.has(x))));
  };

  const sectionItems = (id: string, keys: string[], files: FileChange[]): ContextMenuEntry[] => [
    {
      label: 'Select all',
      icon: <Check size={15} strokeWidth={1.75} />,
      onSelect: () => setMany(keys, true)
    },
    {
      label: 'Deselect all',
      icon: <span />,
      onSelect: () => setMany(keys, false)
    },
    'separator',
    {
      label: 'Expand all',
      icon: <ListTree size={15} strokeWidth={1.75} />,
      onSelect: () => expandSection(id)
    },
    {
      label: 'Collapse all',
      icon: <ListCollapse size={15} strokeWidth={1.75} />,
      onSelect: () => collapseSection(id)
    },
    'separator',
    {
      label: `Rollback ${pluralize(files.length)}...`,
      icon: <Undo2 size={15} strokeWidth={1.75} />,
      danger: true,
      onSelect: () => setRollback(files)
    },
    ...ignoreEntries(files)
  ];

  const ignoreEntries = (files: FileChange[]): ContextMenuEntry[] => {
    const repo = files[0]?.repo;
    if (!repo || files.some((f) => f.repo !== repo)) return [];
    return [gitEntry(repo, files.map((f) => f.path))];
  };

  const addIgnore = (repo: string, patterns: string[], local: boolean) => {
    patterns
      .reduce((chain, pattern) => chain.then(() => gitAddIgnore(repo, pattern, local)), Promise.resolve())
      .then(() => fetchStatus(true))
      .catch((err: unknown) => setError(message(err)));
  };

  const closeRollback = () => setRollback(null);

  const confirmRollback = () => {
    if (!rollback || rollbackBusy) return;
    setRollbackBusy(true);
    rollback
      .reduce((chain, file) => chain.then(() => gitRollbackFile(file.repo, file.path)), Promise.resolve())
      .then(() => {
        setRollback(null);
        fetchStatus(true);
      })
      .catch((err: unknown) => setError(message(err)))
      .finally(() => setRollbackBusy(false));
  };

  const rollbackFooter = (
    <>
      <button type="button" className={styles.dlgBtn} onClick={closeRollback}>
        Cancel
      </button>
      <button type="submit" className={`${styles.dlgBtn} ${styles.dlgDanger}`} disabled={rollbackBusy}>
        {rollbackBusy ? 'Working...' : 'Rollback'}
      </button>
    </>
  );

  const gitEntry = (repo: string, patterns: string[]): ContextMenuEntry => ({
    label: 'Git',
    icon: <GitBranch size={15} strokeWidth={1.75} />,
    submenu: [
      { label: 'Add local exclude', onSelect: () => addIgnore(repo, patterns, true) },
      { label: 'Add to .gitignore', onSelect: () => addIgnore(repo, patterns, false) }
    ]
  });

  const menuItems = (repo: string, rel: string, file: FileChange | null, files: FileChange[]): ContextMenuEntry[] => {
    if (!file)
      return [
        {
          label: `Rollback ${pluralize(files.length)}...`,
          icon: <Undo2 size={15} strokeWidth={1.75} />,
          danger: true,
          onSelect: () => setRollback(files)
        },
        'separator',
        gitEntry(repo, [rel])
      ];
    return [
      {
        label: 'Commit file',
        icon: <Check size={15} strokeWidth={1.75} />,
        onSelect: () => setMany([changeKey(file)], true)
      },
      {
        label: 'Show diff',
        icon: <GitCompareArrows size={15} strokeWidth={1.75} />,
        onSelect: () => openDiff(changeKey(file))
      },
      {
        label: 'Edit file',
        icon: <PenLine size={15} strokeWidth={1.75} />,
        shortcut: formatCombo(getBinding('diff.editFile')),
        onSelect: () => onOpenFile(absPath(repo, file.path))
      },
      {
        label: 'Rollback...',
        icon: <Undo2 size={15} strokeWidth={1.75} />,
        danger: true,
        onSelect: () => setRollback([file])
      },
      'separator',
      {
        label: 'Copy path',
        icon: <Copy size={15} strokeWidth={1.75} />,
        onSelect: () => writeClipboard(absPath(repo, rel))
      },
      { label: 'Copy relative path', icon: <span />, onSelect: () => writeClipboard(rel) },
      'separator',
      {
        label: 'Show in Explorer',
        icon: <FolderOpen size={15} strokeWidth={1.75} />,
        onSelect: () => revealPath(absDir(file))
      },
      'separator',
      gitEntry(repo, [rel])
    ];
  };

  const viewItems: ContextMenuEntry[] = [
    {
      label: 'Directory',
      icon: groupBy === 'directory' ? <Check size={15} strokeWidth={2} /> : <span />,
      onSelect: groupDirectory
    },
    { label: 'Module', icon: groupBy === 'module' ? <Check size={15} strokeWidth={2} /> : <span />, onSelect: groupModule }
  ];

  const toggleAmend = (e: React.ChangeEvent<HTMLInputElement>) => {
    const on = e.target.checked;
    setAmend(on);
    const last = lastCommit.current;
    if (on && last && msg.trim() === '') setMsg(last.body);
    else if (!on && last && msg === last.body) setMsg('');
  };

  const selectedByRepo = (): Array<[string, string[]]> => {
    const groups = new Map<string, string[]>();
    for (const key of selected) {
      const { repo, path } = splitKey(key);
      const list = groups.get(repo);
      if (list) list.push(path);
      else groups.set(repo, [path]);
    }
    return [...groups];
  };

  const commit = (push: boolean) => {
    setBusy(true);
    setError(null);
    selectedByRepo()
      .reduce(
        (chain, [repo, files]) =>
          chain
            .then(() => gitCommit(repo, files, msg, amend))
            .then(() => (push ? gitPushCurrent(repo).then(() => undefined) : undefined)),
        Promise.resolve()
      )
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
    repos
      .filter((repo) => (unpushed[repo.root] ?? 0) > 0)
      .reduce((chain, repo) => chain.then(() => gitPushCurrent(repo.root).then(() => undefined)), Promise.resolve())
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
    void gitLogMessages(primary, 20)
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
    void gitUnpushedCommits(primary)
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
      const keys = collectKeys(node.children);
      const open = () => toggleCollapse(node.id);
      const check = (on: boolean) => setMany(keys, on);
      const files = collectFiles(node.children);
      const menu = (e: React.MouseEvent) => openFolderMenu(e, files[0]?.repo ?? primary, node.dir, files);

      return (
        <div key={node.id}>
          <div className={styles.row} style={{ paddingLeft: pad }} onClick={open} onContextMenu={menu}>
            {shut ? (
              <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} />
            ) : (
              <ChevronDown size={12} strokeWidth={2.5} className={styles.caret} />
            )}
            <TriCheckbox state={triState(keys)} onChange={check} />
            <FileIcon dir open={!shut} size={14} />
            <span className={styles.name}>{node.name}</span>
          </div>
          {!shut && node.children.map((child) => renderNode(child, depth + 1))}
        </div>
      );
    }

    const file = node.change;
    const key = changeKey(file);
    const kind = statusKey(file);
    const pick = () => toggle(key);
    const show = () => openDiff(key);
    const menu = (e: React.MouseEvent) => openFileMenu(e, file);

    return (
      <div
        key={node.id}
        className={styles.row}
        style={{ paddingLeft: pad + 16 }}
        title={file.path}
        onClick={show}
        onContextMenu={menu}
        data-active={key === active}
      >
        <input type="checkbox" checked={selected.has(key)} onChange={pick} onClick={stopClick} />
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.name}
          style={{ color: STATUS_COLOR[kind], textDecoration: kind === 'deleted' ? 'line-through' : undefined }}
        >
          {file.name}
        </span>
      </div>
    );
  };

  const flatRow = (file: FileChange, depth: number) => {
    const key = changeKey(file);
    const kind = statusKey(file);
    const pick = () => toggle(key);
    const show = () => openDiff(key);
    const menu = (e: React.MouseEvent) => openFileMenu(e, file);
    return (
      <div
        key={key}
        className={styles.row}
        style={{ paddingLeft: 43 + depth * 14 }}
        title={file.path}
        onClick={show}
        onContextMenu={menu}
        data-active={key === active}
      >
        <input type="checkbox" checked={selected.has(key)} onChange={pick} onClick={stopClick} />
        <FileIcon name={file.name} size={14} />
        <span
          className={styles.fileName}
          style={{ color: STATUS_COLOR[kind], textDecoration: kind === 'deleted' ? 'line-through' : undefined }}
        >
          {file.name}
        </span>
        {file.dir && <span className={styles.dir}>{displayDir(file.dir)}</span>}
      </div>
    );
  };

  const repoBody = (id: string, repo: RepoEntry, files: FileChange[], depth: number): React.ReactNode => (
    <React.Fragment key={repo.root}>
      {groupBy === 'directory'
        ? buildDirTree(files, repoKey(id, repo.root)).map((node) => renderNode(node, depth))
        : files.map((file) => flatRow(file, depth - 1))}
    </React.Fragment>
  );

  const repoGroup = (id: string, repo: RepoEntry, files: FileChange[]) => {
    const rid = repoKey(id, repo.root);
    const shut = !needle && collapsed.has(rid);
    const keys = files.map(changeKey);
    const open = () => toggleCollapse(rid);
    const check = (on: boolean) => setMany(keys, on);
    const menu = (e: React.MouseEvent) => openFolderMenu(e, repo.root, '.', files);

    return (
      <div key={rid}>
        <div className={styles.row} style={{ paddingLeft: 22 }} onClick={open} onContextMenu={menu}>
          {shut ? (
            <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} />
          ) : (
            <ChevronDown size={12} strokeWidth={2.5} className={styles.caret} />
          )}
          <TriCheckbox state={triState(keys)} onChange={check} />
          <FileIcon dir open={!shut} size={14} />
          <span className={styles.fileName}>{repo.name}</span>
          <span className={styles.count}>{pluralize(files.length)}</span>
          {repo.branch && <span className={styles.branch}>{repo.branch}</span>}
        </div>
        {!shut && repoBody(id, repo, files, 2)}
      </div>
    );
  };

  const section = (id: string, label: string) => {
    const all = sectionFiles(id);
    if (all.length === 0) return null;
    const shown = all.filter(matches);
    const keys = shown.map(changeKey);
    const shut = !needle && collapsed.has(id);
    const open = () => toggleCollapse(id);
    const check = (on: boolean) => setMany(keys, on);
    const menu = (e: React.MouseEvent) => {
      e.preventDefault();
      e.stopPropagation();
      setSectionMenu({ x: e.clientX, y: e.clientY, id, keys, files: shown });
    };

    const groups = repos
      .map((repo) => [repo, filesOf(repo.root, id).filter(matches)] as const)
      .filter(([, files]) => files.length > 0);

    return (
      <div>
        <div className={styles.sectionHead} onClick={open} onContextMenu={menu}>
          {shut ? (
            <ChevronRight size={12} strokeWidth={2.5} className={styles.caret} />
          ) : (
            <ChevronDown size={12} strokeWidth={2.5} className={styles.caret} />
          )}
          <TriCheckbox state={triState(keys)} onChange={check} />
          <span className={styles.sectionLabel}>{label}</span>
          <span className={styles.count}>{pluralize(all.length)}</span>
        </div>
        {!shut &&
          (multi
            ? groups.map(([repo, files]) => repoGroup(id, repo, files))
            : groups.map(([repo, files]) => repoBody(id, repo, files, 1)))}
      </div>
    );
  };

  const ready = repos.length > 0 && Object.keys(statuses).length > 0;
  const clean = ready && SECTIONS.every(([id]) => sectionFiles(id).length === 0);

  const blocked = Boolean(error && !ready);
  const friendly = error && error.includes('not a git repository') ? 'Not a git repository' : error;
  const ahead = Object.values(unpushed).reduce((sum, n) => sum + n, 0);

  const showChanges = () => setView('changes');
  const showHistory = () => setView('history');

  return (
    <div className={styles.root}>
      <div className={styles.views}>
        <button className={styles.view} onClick={showChanges} data-active={view === 'changes' || undefined}>
          Changes
        </button>
        <button className={styles.view} onClick={showHistory} data-active={view === 'history' || undefined}>
          History
        </button>
      </div>

      {view === 'history' && <Log root={primary} active={active} onOpenDiff={openCommitDiff} />}

      <div className={styles.pane} style={{ display: view === 'changes' ? undefined : 'none' }}>
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
          <button className={styles.tool} onClick={collapseAll} disabled={blocked} title="Collapse all" aria-label="Collapse all">
            <ListCollapse size={12} strokeWidth={2} />
          </button>
        </div>

        <div ref={listRef} className={styles.list} tabIndex={-1} onKeyDown={onListKeys}>
          {!ready && !error && (
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
          {ready && SECTIONS.map(([id, label]) => <React.Fragment key={id}>{section(id, label)}</React.Fragment>)}
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
            <button className={styles.primary} onClick={doCommit} disabled={!canCommit} data-amend={amend || undefined}>
              {busy ? 'Working...' : amend ? 'Amend Commit' : 'Commit'}
            </button>
            {clean && ahead > 0 ? (
              <button className={styles.secondary} onClick={push} disabled={pushing}>
                <ArrowUp size={12} strokeWidth={2} />
                {pushing ? 'Pushing...' : `Push ${ahead} ${ahead === 1 ? 'commit' : 'commits'}`}
              </button>
            ) : (
              <button className={styles.secondary} onClick={doCommitPush} disabled={!canCommit}>
                {amend ? 'Amend Commit and Push...' : 'Commit and Push...'}
              </button>
            )}
          </div>
        </div>
      </div>

      {viewMenu && <ContextMenu x={viewMenu.x} y={viewMenu.y} items={viewItems} onClose={closeViewMenu} />}
      {fileMenu && (
        <ContextMenu
          x={fileMenu.x}
          y={fileMenu.y}
          items={menuItems(fileMenu.repo, fileMenu.rel, fileMenu.file, fileMenu.files)}
          onClose={closeFileMenu}
        />
      )}
      {sectionMenu && (
        <ContextMenu
          x={sectionMenu.x}
          y={sectionMenu.y}
          items={sectionItems(sectionMenu.id, sectionMenu.keys, sectionMenu.files)}
          onClose={closeSectionMenu}
        />
      )}
      {rollback && (
        <Dialog title="Rollback changes" footer={rollbackFooter} onClose={closeRollback} onSubmit={confirmRollback}>
          <p className={styles.confirmText}>{rollbackText(rollback)}</p>
        </Dialog>
      )}
    </div>
  );
};

export default GitTab;
