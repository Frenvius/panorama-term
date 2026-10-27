import React from 'react';
import { X, Plus, Search, Loader2, RefreshCw, ArrowUpRight, ChevronRight, ChevronDown, FolderClosed, ArrowDownLeft } from 'lucide-react';

import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { BranchLeaf, BranchNode, BranchAction, BranchSnapshot, RepoOperationAction } from '~/domain/interfaces/git.interface';
import ContextMenu from '~/components/commons/ContextMenu';
import BranchDiff from '~/components/Canvas/TileFrame/BranchMenu/BranchDiff';
import BranchDialog, { type DialogState } from '~/components/Canvas/TileFrame/BranchMenu/BranchDialog';
import BranchRow, { type BranchRowHandlers } from '~/components/Canvas/TileFrame/BranchMenu/BranchRow';
import OperationBanner from '~/components/Canvas/TileFrame/BranchMenu/OperationBanner';
import { useRepoState } from '~/usecase/hooks/useRepoState';
import { gitCompareWithCurrent } from '~/adapter/git/git.client';
import { useBranchActions } from '~/usecase/hooks/useBranchActions';
import { EMPTY_TREES, buildTrees, filterTrees, labelMatches, visibleLeaves } from '~/usecase/util/branchTree';

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
  const [diffBranch, setDiffBranch] = React.useState<string | null>(null);
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const bodyRef = React.useRef<HTMLDivElement>(null);
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
      if (e.key === 'Escape' && !menu && !dialog && !diffBranch) onClose();
    };
    document.addEventListener('pointerdown', onOutside, true);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('pointerdown', onOutside, true);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [onClose, menu, dialog]);

  const trees = React.useMemo(() => (snapshot ? buildTrees(snapshot) : EMPTY_TREES), [snapshot]);
  const filtered = React.useMemo(() => filterTrees(trees, query), [trees, query]);

  const current = snapshot?.current ?? '';
  const filtering = query.trim().length > 0;

  const isOpen = (folderKey: string) => filtering || !collapsed.has(folderKey);
  const leaves = visibleLeaves(
    [
      ['favorites', filtered.favorites],
      ['recent', filtered.recent],
      ['local', filtered.local],
      ['remote', filtered.remote]
    ],
    isOpen
  );
  const selected =
    leaves.find((l) => l.id === selectedId) ?? (filtering && selectedId === null ? leaves[0] : undefined);
  const selectedKey = selected?.id;

  const rowElement = (id: string) => bodyRef.current?.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(id)}"]`);

  React.useEffect(() => {
    const body = bodyRef.current;
    const row = selectedKey ? rowElement(selectedKey) : null;
    if (!body || !row) return;
    if (row.offsetTop < body.scrollTop) body.scrollTop = row.offsetTop;
    else if (row.offsetTop + row.offsetHeight > body.scrollTop + body.clientHeight) {
      body.scrollTop = row.offsetTop + row.offsetHeight - body.clientHeight;
    }
  }, [selectedKey]);

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
      case 'update-rebase':
        void actions.pullRebase();
        return;
      case 'checkout-rebase':
        void actions.checkoutAndRebase(action.branch);
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
      case 'show-diff':
        setDiffBranch(action.branch);
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
    if (!leaf.isCurrent && !isRemote && current) {
      items.push({
        label: `Checkout and rebase onto '${current}'`,
        onSelect: () => dispatch({ type: 'checkout-rebase', branch: name })
      });
    }
    items.push({ label: `New branch from '${short}'...`, onSelect: () => dispatch({ type: 'new-from', branch: name }) });

    if (!leaf.isCurrent) {
      items.push('separator');
      items.push({ label: 'Compare with current', onSelect: () => dispatch({ type: 'compare', branch: name }) });
      items.push({ label: 'Show diff with working tree', onSelect: () => dispatch({ type: 'show-diff', branch: name }) });
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
      items.push({ label: 'Pull (merge)', onSelect: () => dispatch({ type: 'update' }) });
      items.push({ label: 'Pull with rebase', onSelect: () => dispatch({ type: 'update-rebase' }) });
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
  const closeDiff = () => setDiffBranch(null);
  const clearError = () => onError(null);
  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setSelectedId(null);
  };

  const openMenu = (leaf: BranchLeaf, rect: DOMRect) => {
    if (busy) return;
    setMenu({ leaf, x: rect.right - 4, y: rect.top });
  };

  const rowHandlers: BranchRowHandlers = {
    onOpen: openMenu,
    onFavorite: (leaf) => dispatch({ type: 'toggle-favorite', branch: leaf.fullName })
  };

  const moveSelection = (step: number) => {
    if (leaves.length === 0) return;
    const index = selected ? leaves.indexOf(selected) : -1;
    const next = index === -1 ? (step > 0 ? 0 : leaves.length - 1) : (index + step + leaves.length) % leaves.length;
    setSelectedId(leaves[next].id);
  };

  const openSelectedMenu = () => {
    const rect = selected && rowElement(selected.id)?.getBoundingClientRect();
    if (selected && rect) openMenu(selected.leaf, rect);
  };

  const activateSelected = () => {
    if (!selected || busy) return;
    if (selected.leaf.isCurrent) openSelectedMenu();
    else dispatch({ type: 'checkout', branch: selected.leaf.fullName });
  };

  const onSearchKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
    const keys: Record<string, () => void> = {
      ArrowDown: () => moveSelection(1),
      ArrowUp: () => moveSelection(-1),
      ArrowRight: openSelectedMenu,
      Enter: activateSelected
    };
    const run = keys[e.key];
    const caretAtEnd = e.currentTarget.selectionStart === e.currentTarget.value.length;
    if (!run || (e.key === 'ArrowRight' && (!selected || !caretAtEnd))) return;
    e.preventDefault();
    run();
  };
  const newBranch = () => setDialog({ kind: 'create' });

  const stopEvent = (e: React.SyntheticEvent) => e.stopPropagation();
  const stopContext = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
  };

  const renderNode = (section: string, node: BranchNode, depth: number): React.ReactNode => {
    if (node.kind === 'folder') {
      const shut = !isOpen(node.key);
      const toggle = () => toggleFolder(node.key);

      return (
        <div key={node.key}>
          <button className={styles.row} style={{ paddingLeft: 8 + depth * 12 }} onClick={toggle}>
            {shut ? <ChevronRight size={12} strokeWidth={2.5} /> : <ChevronDown size={12} strokeWidth={2.5} />}
            <FolderClosed size={13} strokeWidth={1.75} className={styles.folder} />
            <span className={styles.name}>{node.label}</span>
          </button>
          {!shut && node.children.map((child) => renderNode(section, child, depth + 1))}
        </div>
      );
    }

    const id = `${section}:${node.key}`;
    const working = busy?.branch === node.data.fullName;

    return (
      <BranchRow
        id={id}
        key={node.key}
        depth={depth}
        leaf={node.data}
        label={node.label}
        handlers={rowHandlers}
        matches={filtering ? labelMatches(query, node.data.fullName, node.label) : []}
        state={{ working, dimmed: Boolean(busy) && !working, selected: id === selectedKey }}
      />
    );
  };

  const section = (id: string, label: string, nodes: BranchNode[], hint?: string) => {
    if (nodes.length === 0 && !hint) return null;
    return (
      <div className={styles.section}>
        <div className={styles.sectionLabel}>{label}</div>
        {nodes.length > 0 ? nodes.map((node) => renderNode(id, node, 0)) : <div className={styles.hint}>{hint}</div>}
      </div>
    );
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
          <input value={query} onChange={onQuery} onKeyDown={onSearchKey} placeholder="Search branches" autoFocus />
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

        <div ref={bodyRef} className={styles.body}>
          {loading && !snapshot ? (
            <div className={styles.hint}>Loading branches...</div>
          ) : (
            <>
              {section('favorites', 'Favorites', filtered.favorites)}
              {section('recent', 'Recent', filtered.recent)}
              {section('local', 'Local', filtered.local, filtering ? 'No local branches match.' : 'No local branches.')}
              {section('remote', 'Remote', filtered.remote, filtering ? 'No remote branches match.' : 'No remote branches.')}
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

      {diffBranch && (
        <div data-branch-overlay onPointerDown={stopEvent} onContextMenu={stopContext}>
          <BranchDiff root={repo?.root ?? cwd} branch={diffBranch} onOpened={onClose} onClose={closeDiff} />
        </div>
      )}
    </>
  );
};

export default BranchMenu;
