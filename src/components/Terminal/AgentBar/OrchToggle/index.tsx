import React from 'react';
import { Plus, Workflow } from 'lucide-react';

import styles from './styles.module.scss';
import { normalizeRepo } from '~/usecase/util/orch';
import { useOrchCoordinator, coordinatorActions } from '~/usecase/hooks/useOrchCoordinator';

interface OrchToggleProps {
  tileId: string;
  cwd?: string;
}

const OrchToggle = ({ tileId, cwd }: OrchToggleProps) => {
  const { on, error, toggle, allRuns } = useOrchCoordinator(tileId);
  const [menuOpen, setMenuOpen] = React.useState(false);
  const menuRef = React.useRef<HTMLDivElement>(null);

  const title = error ? `Orchestrate mode - ${error}` : 'Orchestrate mode';

  const normCwd = cwd ? normalizeRepo(cwd) : '';

  const projectBoards = allRuns.filter((s) => {
    if (s.run.status === 'completed' || s.run.status === 'cancelled') return false;
    if (s.run.coordinatorTile) return false;
    if (!normCwd) return false;
    return normalizeRepo(s.run.repo) === normCwd;
  });

  React.useEffect(() => {
    if (!menuOpen) return;
    const onDown = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    window.addEventListener('mousedown', onDown, true);
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('mousedown', onDown, true);
      window.removeEventListener('keydown', onKey, true);
    };
  }, [menuOpen]);

  const handleClick = () => {
    if (on) {
      toggle();
      return;
    }
    if (projectBoards.length === 0) {
      toggle();
      return;
    }
    setMenuOpen((o) => !o);
  };

  const handleNewBoard = () => {
    setMenuOpen(false);
    toggle();
  };

  const handleCoordinate = (runId: string) => {
    setMenuOpen(false);
    coordinatorActions.attachToRun(tileId, runId);
  };

  return (
    <div className={styles.wrap} ref={menuRef}>
      <button type="button" title={title} aria-pressed={on} onClick={handleClick} className={styles.toggle}>
        <Workflow size={14} strokeWidth={1.75} />
      </button>
      {menuOpen && (
        <div className={styles.popover}>
          <button type="button" className={styles.popoverItem} onClick={handleNewBoard}>
            <Plus size={11} strokeWidth={2} />
            New board
          </button>
          {projectBoards.map((s) => {
            const handleCoord = () => handleCoordinate(s.run.id);
            return (
              <button key={s.run.id} type="button" className={styles.popoverItem} onClick={handleCoord}>
                <Workflow size={11} strokeWidth={1.75} />
                Coordinate {s.run.title}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default OrchToggle;
