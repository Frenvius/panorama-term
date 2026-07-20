import React from 'react';
import { Star, Plus, Search, GitBranch, RefreshCw, ArrowUpRight, ChevronRight, ChevronDown, FolderClosed, ArrowDownLeft } from 'lucide-react';

import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { BranchLeaf, BranchNode, LocalBranch, BranchAction, BranchSnapshot } from '~/domain/interfaces/git.interface';
import ContextMenu from '~/components/commons/ContextMenu';
import BranchDialog, { type DialogState } from '~/components/Canvas/TileFrame/BranchMenu/BranchDialog';
import { buildTrees, filterTree } from '~/usecase/util/branchTree';
import {
  gitFetch,
  gitCheckout,
  gitMergeBranch,
  gitRebaseOnto,
  gitSetUpstream,
  gitPushCurrent,
  gitDeleteBranch,
  gitCreateBranch,
  gitRenameBranch,
  gitUpdateBranch,
  gitToggleBranchFavorite
} from '~/adapter/git/git.client';

import styles from './styles.module.scss';

interface BranchMenuProps {
  k: number;
  cwd: string;
  anchor: { x: number; y: number };
  zIndex: number;
  snapshot: BranchSnapshot | null;
  loading: boolean;
  error: string | null;
  onClose: () => void;
  onSnapshot: (snap: BranchSnapshot) => void;
  onError: (error: string | null) => void;
}

interface MenuState {
  leaf: BranchLeaf;
  x: number;
  y: number;
}

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const head = (snap: BranchSnapshot | null): LocalBranch | undefined => snap?.local.find((b) => b.is_current);

const commits = (n: number): string => `${n} commit${n === 1 ? '' : 's'}`;

const summarize = (kind: 'update' | 'push', before?: LocalBranch, after?: LocalBranch): string => {
  const moved = kind === 'update' ? (before?.behind ?? 0) - (after?.behind ?? 0) : (before?.ahead ?? 0) - (after?.ahead ?? 0);
  const target = after?.upstream ?? before?.upstream ?? 'remote';
  if (moved <= 0) return 'Already up to date.';
  return kind === 'update' ? `Pulled ${commits(moved)} from ${target}.` : `Pushed ${commits(moved)} to ${target}.`;
};

const BranchMenu = ({ k, cwd, anchor, zIndex, snapshot, loading, error, onClose, onSnapshot, onError }: BranchMenuProps) => {
  const [query, setQuery] = React.useState('');
  const [collapsed, setCollapsed] = React.useState<Set<string>>(() => new Set());
  const [menu, setMenu] = React.useState<MenuState | null>(null);
  const [dialog, setDialog] = React.useState<DialogState | null>(null);
  const [fetching, setFetching] = React.useState(false);
  const [notice, setNotice] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState<'update' | 'push' | null>(null);
  const rootRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    const onOutside = (e: PointerEvent) => {
      const target = e.target as Node;
      if (rootRef.current?.contains(target)) return;
      if ((target as Element).closest?.('[data-branch-overlay], [data-ctx-menu]')) return;
      onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !menu && !dialog) onClose();
    };
    document.addEventListener('pointerdown', onOutside, true);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('pointerdown', onOutside, true);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [onClose, menu, dialog]);

  const trees = React.useMemo(
    () => (snapshot ? buildTrees(snapshot) : { local: [], remote: [], favorites: [] }),
    [snapshot]
  );

  const current = snapshot?.current ?? '';
  const filtering = query.trim().length > 0;

  const run = (task: Promise<BranchSnapshot>): Promise<BranchSnapshot> =>
    task
      .then((snap) => {
        onSnapshot(snap);
        onError(null);
        return snap;
      })
      .catch((err: unknown) => {
        onError(message(err));
        throw err;
      });

  const fetchAll = () => {
    setFetching(true);
    void run(gitFetch(cwd))
      .catch(() => {})
      .finally(() => setFetching(false));
  };

  const runPending = (kind: 'update' | 'push', task: Promise<BranchSnapshot>) => {
    const before = head(snapshot);
    setPending(kind);
    setNotice(null);
    void run(task)
      .then((snap) => setNotice(summarize(kind, before, head(snap))))
      .catch(() => {})
      .finally(() => setPending(null));
  };

  const updateProject = () => runPending('update', gitUpdateBranch(cwd, false));
  const pushCurrent = () => runPending('push', gitPushCurrent(cwd));

  const create = (name: string, checkout: boolean, overwrite: boolean, startPoint?: string) =>
    run(gitCreateBranch(cwd, name, checkout, overwrite, startPoint));

  const rename = (oldName: string, newName: string) => run(gitRenameBranch(cwd, oldName, newName));

  const dispatch = (action: BranchAction) => {
    switch (action.type) {
      case 'checkout':
        void run(gitCheckout(cwd, action.branch)).catch(() => {});
        return;
      case 'toggle-favorite':
        void run(gitToggleBranchFavorite(cwd, action.branch)).catch(() => {});
        return;
      case 'update':
        void run(gitUpdateBranch(cwd, false)).catch(() => {});
        return;
      case 'push':
        void run(gitPushCurrent(cwd)).catch(() => {});
        return;
      case 'merge':
        void run(gitMergeBranch(cwd, action.branch)).catch(() => {});
        return;
      case 'rebase':
        void run(gitRebaseOnto(cwd, action.branch)).catch(() => {});
        return;
      case 'set-upstream':
        void run(gitSetUpstream(cwd, action.branch, action.upstream)).catch(() => {});
        return;
      case 'compare':
        setDialog({ kind: 'compare', branch: action.branch });
        return;
      case 'new-from':
        setDialog({ kind: 'create', startPoint: action.branch });
        return;
      case 'rename':
        setDialog({ kind: 'rename', branch: action.branch });
        return;
      case 'delete':
        setDialog({
          kind: 'confirm',
          title: action.isRemote ? 'Delete remote branch' : 'Delete branch',
          message: action.isRemote
            ? `Delete remote branch '${action.branch}'? This pushes a delete to the remote.`
            : `Delete branch '${action.branch}'? Unmerged commits may be lost.`,
          confirmLabel: 'Delete',
          run: () => run(gitDeleteBranch(cwd, action.branch, action.isRemote))
        });
    }
  };

  const menuItems = (leaf: BranchLeaf): ContextMenuEntry[] => {
    const isRemote = leaf.kind === 'remote';
    const name = leaf.fullName;
    const short = name.split('/').slice(isRemote ? 1 : 0).join('/') || name;
    const items: ContextMenuEntry[] = [];

    if (!leaf.isCurrent) items.push({ label: 'Checkout', onSelect: () => dispatch({ type: 'checkout', branch: name }) });
    items.push({ label: `New branch from '${short}'...`, onSelect: () => dispatch({ type: 'new-from', branch: name }) });

    if (!leaf.isCurrent) {
      items.push('separator');
      items.push({ label: 'Compare with current', onSelect: () => dispatch({ type: 'compare', branch: name }) });
      items.push('separator');
      items.push({
        label: current ? `Merge '${short}' into '${current}'` : `Merge '${short}' into current`,
        onSelect: () => dispatch({ type: 'merge', branch: name })
      });
      items.push({
        label: current ? `Rebase '${current}' onto '${short}'` : `Rebase current onto '${short}'`,
        onSelect: () => dispatch({ type: 'rebase', branch: name })
      });
    } else {
      items.push('separator');
      items.push({ label: 'Update', onSelect: () => dispatch({ type: 'update' }) });
      items.push({ label: 'Push', onSelect: () => dispatch({ type: 'push' }) });
    }

    if (!isRemote) {
      const submenu: ContextMenuEntry[] = (snapshot?.remotes ?? []).map((r) => {
        const full = `${r.remote}/${r.branch}`;
        return { label: full, onSelect: () => dispatch({ type: 'set-upstream', branch: name, upstream: full }) };
      });
      if (leaf.upstream) {
        submenu.push('separator');
        submenu.push({
          label: "Don't track",
          onSelect: () => dispatch({ type: 'set-upstream', branch: name, upstream: null })
        });
      }
      items.push('separator');
      items.push({
        label: leaf.upstream ? `Tracked branch '${leaf.upstream}'` : 'Set tracked branch',
        submenu: submenu.length > 0 ? submenu : [{ label: 'No remote branches', disabled: true }]
      });
    }

    items.push('separator');
    if (!isRemote) {
      items.push({ label: 'Rename...', onSelect: () => dispatch({ type: 'rename', branch: name }) });
    }
    if (!leaf.isCurrent) {
      items.push({
        label: isRemote ? 'Delete (remote)' : 'Delete',
        danger: true,
        onSelect: () => dispatch({ type: 'delete', branch: name, isRemote })
      });
    }

    items.push('separator');
    items.push({
      label: leaf.isFavorite ? 'Unmark as favorite' : 'Mark as favorite',
      onSelect: () => dispatch({ type: 'toggle-favorite', branch: name })
    });

    return items;
  };

  const toggleFolder = (key: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  };

  const closeMenu = () => setMenu(null);
  const closeDialog = () => setDialog(null);
  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => setQuery(e.target.value);
  const newBranch = () => setDialog({ kind: 'create' });

  const stopEvent = (e: React.SyntheticEvent) => e.stopPropagation();
  const stopContext = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const renderNode = (node: BranchNode, depth: number): React.ReactNode => {
    if (node.kind === 'folder') {
      const shut = !filtering && collapsed.has(node.key);
      const toggle = () => toggleFolder(node.key);

      return (
        <div key={node.key}>
          <button className={styles.row} style={{ paddingLeft: 8 + depth * 12 }} onClick={toggle}>
            {shut ? <ChevronRight size={12} strokeWidth={2.5} /> : <ChevronDown size={12} strokeWidth={2.5} />}
            <FolderClosed size={13} strokeWidth={1.75} className={styles.folder} />
            <span className={styles.name}>{node.label}</span>
          </button>
          {!shut && node.children.map((child) => renderNode(child, depth + 1))}
        </div>
      );
    }

    const leaf = node.data;
    const track = [leaf.ahead > 0 && `+${leaf.ahead}`, leaf.behind > 0 && `-${leaf.behind}`]
      .filter(Boolean)
      .join(' ');

    const open = (e: React.MouseEvent) => {
      e.preventDefault();
      const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
      setMenu({ leaf, x: rect.right - 4, y: rect.top });
    };
    const favorite = (e: React.MouseEvent) => {
      e.stopPropagation();
      dispatch({ type: 'toggle-favorite', branch: leaf.fullName });
    };

    return (
      <div
        key={node.key}
        className={styles.row}
        style={{ paddingLeft: 8 + depth * 12 }}
        onClick={open}
        onContextMenu={open}
        data-current={leaf.isCurrent || undefined}
      >
        <button className={styles.star} onClick={favorite} aria-label="Toggle favorite" data-on={leaf.isFavorite || undefined}>
          {leaf.isFavorite ? <Star size={12} fill="currentColor" /> : <GitBranch size={12} strokeWidth={1.75} />}
        </button>
        <span className={styles.name}>{node.label}</span>
        {track && <span className={styles.track}>{track}</span>}
        {leaf.upstream && <span className={styles.upstream}>{leaf.upstream}</span>}
      </div>
    );
  };

  const section = (label: string, nodes: BranchNode[], hint?: string) => {
    if (nodes.length === 0 && !hint) return null;
    return (
      <div className={styles.section}>
        <div className={styles.sectionLabel}>{label}</div>
        {nodes.length > 0 ? nodes.map((node) => renderNode(node, 0)) : <div className={styles.hint}>{hint}</div>}
      </div>
    );
  };

  const filtered = {
    favorites: filterTree(trees.favorites, query),
    local: filterTree(trees.local, query),
    remote: filterTree(trees.remote, query)
  };

  return (
    <>
      <div
        ref={rootRef}
        className={styles.panel}
        style={{ top: anchor.y, left: anchor.x, zIndex, transform: `scale(${k})`, transformOrigin: 'top left' }}
        onPointerDown={stopEvent}
        onContextMenu={stopContext}
        onWheel={stopEvent}
        onDoubleClick={stopEvent}
      >
        <div className={styles.search}>
          <Search size={12} strokeWidth={2} />
          <input value={query} onChange={onQuery} placeholder="Search branches" autoFocus />
          <button
            className={styles.action}
            onClick={updateProject}
            disabled={pending !== null}
            data-tooltip={pending === 'update' ? 'Pulling...' : 'Pull'}
            aria-label="Pull"
          >
            <ArrowDownLeft size={12} strokeWidth={2} />
          </button>
          <button
            className={styles.action}
            onClick={pushCurrent}
            disabled={pending !== null}
            data-tooltip={pending === 'push' ? 'Pushing...' : 'Push'}
            aria-label="Push"
          >
            <ArrowUpRight size={12} strokeWidth={2} />
          </button>
          <button className={styles.action} onClick={fetchAll} disabled={fetching} data-tooltip="Fetch" aria-label="Fetch">
            <RefreshCw size={12} strokeWidth={2} className={fetching ? styles.spinning : undefined} />
          </button>
        </div>

        <button className={styles.new} onClick={newBranch}>
          <Plus size={13} strokeWidth={2} />
          New branch...
        </button>

        {error && <div className={styles.error}>{error}</div>}
        {!error && notice && <div className={styles.notice}>{notice}</div>}

        <div className={styles.body}>
          {loading && !snapshot ? (
            <div className={styles.hint}>Loading branches...</div>
          ) : (
            <>
              {section('Favorites', filtered.favorites)}
              {section('Local', filtered.local, filtering ? 'No local branches match.' : 'No local branches.')}
              {section('Remote', filtered.remote, filtering ? 'No remote branches match.' : 'No remote branches.')}
            </>
          )}
        </div>
      </div>

      {menu && (
        <div data-branch-overlay onPointerDown={stopEvent} onContextMenu={stopContext}>
          <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.leaf)} onClose={closeMenu} />
        </div>
      )}

      {dialog && (
        <div data-branch-overlay onPointerDown={stopEvent} onContextMenu={stopContext}>
          <BranchDialog
            state={dialog}
            cwd={cwd}
            current={current}
            onClose={closeDialog}
            onCreate={create}
            onRename={rename}
          />
        </div>
      )}
    </>
  );
};

export default BranchMenu;
