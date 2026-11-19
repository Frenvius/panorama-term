import React from 'react';
import { X, Plus } from 'lucide-react';

import { useWorkspace } from '~/usecase/context/WorkspaceContext';

import styles from './styles.module.scss';

const WorkspaceBar = () => {
  const { workspaces, activeId, switchWorkspace, createWorkspace, deleteWorkspace } = useWorkspace();
  const [open, setOpen] = React.useState(false);
  const [naming, setNaming] = React.useState(false);
  const [draft, setDraft] = React.useState('');
  const rootRef = React.useRef<HTMLDivElement>(null);

  const active = workspaces.find((w) => w.id === activeId) ?? null;

  const toggle = () => {
    setOpen((v) => !v);
    setNaming(false);
  };

  React.useEffect(() => {
    if (!open) return;
    const onOutside = (e: PointerEvent) => {
      if (rootRef.current?.contains(e.target as Node)) return;
      setOpen(false);
      setNaming(false);
    };
    document.addEventListener('pointerdown', onOutside, true);
    return () => document.removeEventListener('pointerdown', onOutside, true);
  }, [open]);

  const startNaming = () => {
    setDraft('');
    setNaming(true);
  };

  const cancelNaming = () => setNaming(false);

  const commitWorkspace = () => {
    void createWorkspace(draft.trim() || undefined);
    setNaming(false);
    setOpen(false);
  };

  const onNameKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') commitWorkspace();
    if (e.key === 'Escape') cancelNaming();
  };

  return (
    <div ref={rootRef} data-no-drag className={styles.root}>
      <button className={styles.trigger} onClick={toggle} aria-label={active?.name ?? 'Workspace'}>
        <span className={styles.dot} style={{ background: active?.color ?? '#8894a8' }} />
      </button>
      {open && (
        <div className={styles.popover}>
          {workspaces.map((workspace) => {
            const select = () => {
              void switchWorkspace(workspace.id);
              setOpen(false);
            };
            const remove = (e: React.MouseEvent) => {
              e.stopPropagation();
              void deleteWorkspace(workspace.id);
            };

            return (
              <div
                key={workspace.id}
                className={styles.item}
                onClick={select}
                data-active={workspace.id === activeId}
              >
                <span className={styles.dot} style={{ background: workspace.color }} />
                <span className={styles.name}>{workspace.name}</span>
                {workspaces.length > 1 && (
                  <button className={styles.remove} onClick={remove} aria-label="Delete workspace">
                    <X size={13} strokeWidth={2} />
                  </button>
                )}
              </div>
            );
          })}
          {naming ? (
            <input
              className={styles.nameInput}
              value={draft}
              placeholder="Workspace name"
              aria-label="Workspace name"
              onBlur={cancelNaming}
              onKeyDown={onNameKey}
              onChange={(e) => setDraft(e.target.value)}
              autoFocus
            />
          ) : (
            <button className={styles.new} onClick={startNaming}>
              <Plus size={14} strokeWidth={2} />
              New workspace
            </button>
          )}
        </div>
      )}
    </div>
  );
};

export default WorkspaceBar;
