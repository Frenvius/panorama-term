import React from 'react';
import { X, Star, Plus, Search, Loader2, GitBranch, RefreshCw, ArrowUpRight, ChevronRight, ChevronDown, FolderClosed, ArrowDownLeft } from 'lucide-react';

import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { BranchLeaf, BranchNode, BranchAction, BranchSnapshot, RepoOperationAction } from '~/domain/interfaces/git.interface';
import ContextMenu from '~/components/commons/ContextMenu';
import BranchDialog, { type DialogState } from '~/components/Canvas/TileFrame/BranchMenu/BranchDialog';
import OperationBanner from '~/components/Canvas/TileFrame/BranchMenu/OperationBanner';
import { useRepoState } from '~/usecase/hooks/useRepoState';
import { gitCompareWithCurrent } from '~/adapter/git/git.client';
import { buildTrees, filterTree } from '~/usecase/util/branchTree';
import { useBranchActions } from '~/usecase/hooks/useBranchActions';

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

const BranchMenu = ({ k, cwd, anchor, zIndex, snapshot, loading, error, onClose, onSnapshot, onError }: BranchMenuProps) => {
  const [query, setQuery] = React.useState('');
  const [collapsed, setCollapsed] = React.useState<Set<string>>(() => new Set());
  const [menu, setMenu] = React.useState<MenuState | null>(null);
  const [dialog, setDialog] = React.useState<DialogState | null>(null);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const actions = useBranchActions(cwd, snapshot, { onSnapshot, onError });
  const repo = useRepoState(cwd);
  const busy = actions.busy;

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

  const fetchAll = () => void actions.fetchAll();
  const updateProject = () => void actions.update();
  const pushCurrent = () => void actions.push();
  const runOperation = (action: RepoOperationAction) => void actions.operation(action);

  const dispatch = (action: BranchAction) => {
    switch (action.type) {
      case 'checkout':
        void actions.checkout(action.branch);
        return;
      case 'toggle-favorite':
        void actions.toggleFavorite(action.branch);
        return;
      case 'update':
        void actions.update();
        return;
      case 'push':
        void actions.push();
        return;
      case 'merge':
        void actions.merge(action.branch);
        return;
      case 'rebase':
        void actions.rebase(action.branch);
        return;
      case 'set-upstream':
        void actions.setUpstream(action.branch, action.upstream);
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
        void confirmDelete(action.branch, action.isRemote);
    }
  };

  const confirmDelete = async (branch: string, isRemote: boolean) => {
    const run = () => actions.remove(branch, isRemote);
    if (isRemote) {
      setDialog({
        run,
        kind: 'confirm',
        confirmLabel: 'Delete',
        title: 'Delete remote branch',
        message: `Delete remote branch '${branch}'? This pushes a delete to the remote.`
      });
      return;
    }
    const unmerged = await gitCompareWithCurrent(cwd, branch).catch(() => []);
    setDialog({
      run,
      kind: 'confirm',
      title: 'Delete branch',
      confirmLabel: 'Delete',
      commits: unmerged.length > 0 ? unmerged : undefined,
      message:
        unmerged.length > 0
          ? `'${branch}' has ${unmerged.length} commit${unmerged.length === 1 ? '' : 's'} not in '${current}'. They will be lost unless another branch contains them. You can still restore the branch right after deleting it.`
          : `Delete branch '${branch}'? It is fully merged into '${current}'.`
    });
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
  const clearError = () => onError(null);
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

    const working = busy?.branch === leaf.fullName;

    const open = (e: React.MouseEvent) => {
      e.preventDefault();
      if (busy) return;
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
        data-busy={(busy && !working) || undefined}
      >
        {working ? (
          <span className={styles.star}>
            <Loader2 size={12} strokeWidth={2} className={styles.spinning} />
          </span>
        ) : (
          <button className={styles.star} onClick={favorite} aria-label="Toggle favorite" data-on={leaf.isFavorite || undefined}>
            {leaf.isFavorite ? <Star size={12} fill="currentColor" /> : <GitBranch size={12} strokeWidth={1.75} />}
          </button>
        )}
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
            disabled={busy !== null}
            data-tooltip={busy?.op === 'update' ? 'Pulling...' : 'Pull'}
            aria-label="Pull"
          >
            {busy?.op === 'update' ? (
              <Loader2 size={12} strokeWidth={2} className={styles.spinning} />
            ) : (
              <ArrowDownLeft size={12} strokeWidth={2} />
            )}
          </button>
          <button
            className={styles.action}
            onClick={pushCurrent}
            disabled={busy !== null}
            data-tooltip={busy?.op === 'push' ? 'Pushing...' : 'Push'}
            aria-label="Push"
          >
            {busy?.op === 'push' ? (
              <Loader2 size={12} strokeWidth={2} className={styles.spinning} />
            ) : (
              <ArrowUpRight size={12} strokeWidth={2} />
            )}
          </button>
          <button
            className={styles.action}
            onClick={fetchAll}
            disabled={busy !== null}
            data-tooltip={busy?.op === 'fetch' ? 'Fetching...' : 'Fetch'}
            aria-label="Fetch"
          >
            <RefreshCw size={12} strokeWidth={2} className={busy?.op === 'fetch' ? styles.spinning : undefined} />
          </button>
        </div>

        <button className={styles.new} onClick={newBranch} disabled={busy !== null}>
          <Plus size={13} strokeWidth={2} />
          New branch...
        </button>

        {repo?.state && <OperationBanner repo={repo} busy={busy !== null} onAction={runOperation} />}

        {error && (
          <div className={styles.error}>
            <span className={styles.errorText}>{error}</span>
            <button type="button" className={styles.dismiss} onClick={clearError} aria-label="Dismiss error">
              <X size={12} strokeWidth={2} />
            </button>
          </div>
        )}

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
            onCreate={actions.create}
            onRename={actions.rename}
          />
        </div>
      )}
    </>
  );
};

export default BranchMenu;
