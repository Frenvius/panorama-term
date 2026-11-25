import type { PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';
import { Plus, ChevronDown, ChevronRight, TriangleAlert } from 'lucide-react';

import AgentRow from './AgentRow';
import UserInboxRow from './UserInboxRow';
import styles from './styles.module.scss';
import { sortAgents } from '~/usecase/util/agents';
import { deleteAgent } from '~/adapter/agents/agents.client';
import { useAgents, agentActions } from '~/usecase/hooks/useAgents';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';

interface AgentsGroupProps {
  isOpen: boolean;
  onToggle: () => void;
  activeProject: string | null;
  onNewAgent: (cwd?: string) => void;
  onEditAgent: (agent: PersistentAgent) => void;
}

const AgentsGroup = ({ isOpen, onToggle, activeProject, onNewAgent, onEditAgent }: AgentsGroupProps) => {
  const { agents, restartNeededIds } = useAgents();
  const [confirmDelete, setConfirmDelete] = React.useState<string | null>(null);

  const sorted = sortAgents(agents);
  const liveCount = agents.filter((a) => a.live).length;
  const showWarning = liveCount > 6;

  const handleDelete = (id: string) => setConfirmDelete(id);

  const handleConfirmDelete = () => {
    if (!confirmDelete) return;
    const id = confirmDelete;
    setConfirmDelete(null);
    deleteAgent(id).then(() => agentActions.refresh()).catch(() => agentActions.refresh());
  };

  const handleCancelDelete = () => setConfirmDelete(null);

  const handleNewAgent = (e: React.MouseEvent) => {
    e.stopPropagation();
    onNewAgent(activeProject ?? undefined);
  };

  return (
    <>
      <div className={styles.group}>
        <div className={styles.header} onClick={onToggle}>
          <div className={styles.headerMain}>
            <button
              type="button"
              className={styles.caret}
              aria-label={isOpen ? 'Collapse Agents' : 'Expand Agents'}
              aria-expanded={isOpen}
            >
              {isOpen ? <ChevronDown size={12} strokeWidth={2.25} /> : <ChevronRight size={12} strokeWidth={2.25} />}
            </button>
            <span className={styles.title}>Agents</span>
            {showWarning && <TriangleAlert size={12} strokeWidth={2} className={styles.warning} />}
          </div>
          <div className={styles.headerActions}>
            <span className={styles.count}>{agents.length}</span>
            <button type="button" className={styles.addBtn} onClick={handleNewAgent} aria-label="New agent">
              <Plus size={12} strokeWidth={2.25} />
            </button>
          </div>
        </div>
        {isOpen && (
          <div className={styles.rows}>
            <UserInboxRow />
            {sorted.map((agent) => (
              <AgentRow
                key={agent.id}
                agent={agent}
                activeProject={activeProject}
                restartNeeded={restartNeededIds.has(agent.id)}
                onEdit={() => onEditAgent(agent)}
                onDelete={() => handleDelete(agent.id)}
              />
            ))}
          </div>
        )}
      </div>
      {confirmDelete && (
        <ConfirmDialog
          title="Delete agent"
          message="Delete this agent? This cannot be undone."
          confirmLabel="Delete"
          danger
          onConfirm={handleConfirmDelete}
          onCancel={handleCancelDelete}
        />
      )}
    </>
  );
};

export default AgentsGroup;
