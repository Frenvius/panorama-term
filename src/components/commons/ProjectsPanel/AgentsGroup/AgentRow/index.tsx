import type { PersistentAgent } from '~/domain/interfaces/agents.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import React from 'react';

import ContextMenu from '~/components/commons/ContextMenu';
import { startAgent, stopAgent } from '~/adapter/agents/agents.client';
import { agentActions } from '~/usecase/hooks/useAgents';
import { requestShowAgent } from '~/usecase/util/agentRequest';
import { statusToDotKind, isNotResumable } from '~/usecase/util/agents';
import { projectName } from '~/usecase/util/projectLayout';
import styles from './styles.module.scss';

interface AgentRowProps {
  agent: PersistentAgent;
  activeProject: string | null;
  restartNeeded: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

const AgentRow = ({ agent, activeProject, restartNeeded, onEdit, onDelete }: AgentRowProps) => {
  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [showWhenLive, setShowWhenLive] = React.useState(false);

  React.useEffect(() => {
    if (!showWhenLive || !agent.live) return;
    setShowWhenLive(false);
    requestShowAgent({ tileId: agent.tileId, cwd: agent.cwd });
  }, [showWhenLive, agent.live, agent.tileId, agent.cwd]);

  const profileModel = agent.model ? `${agent.profile}:${agent.model}` : agent.profile;
  const showProject = agent.project && agent.project !== activeProject;
  const dotKind = statusToDotKind(agent.status);
  const isBusy = agent.status === 'busy' || agent.status === 'waiting';

  const handleClick = () => {
    if (agent.live) {
      requestShowAgent({ tileId: agent.tileId, cwd: agent.cwd });
      return;
    }
    setShowWhenLive(true);
    startAgent(agent.id).then(() => agentActions.refresh()).catch(() => {
      setShowWhenLive(false);
      agentActions.refresh();
    });
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const handleCloseMenu = () => setMenu(null);

  const handleStart = () => {
    setMenu(null);
    startAgent(agent.id).then(() => agentActions.refresh()).catch(() => agentActions.refresh());
  };

  const handleStop = () => {
    setMenu(null);
    stopAgent(agent.id).then(() => agentActions.refresh()).catch(() => agentActions.refresh());
  };

  const handleForceStop = () => {
    setMenu(null);
    stopAgent(agent.id, true).then(() => agentActions.refresh()).catch(() => agentActions.refresh());
  };

  const handleRestart = () => {
    setMenu(null);
    stopAgent(agent.id, true)
      .then(() => startAgent(agent.id, true))
      .then(() => agentActions.refresh())
      .catch(() => agentActions.refresh());
  };

  const handleEdit = () => {
    setMenu(null);
    onEdit();
  };

  const handleDelete = () => {
    setMenu(null);
    onDelete();
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleClick();
    }
  };

  const menuItems: ContextMenuEntry[] = [];
  if (!agent.live) menuItems.push({ label: 'Start', onSelect: handleStart });
  if (agent.live && !isBusy) menuItems.push({ label: 'Stop', onSelect: handleStop });
  if (agent.live && isBusy) menuItems.push({ label: 'Force stop', onSelect: handleForceStop, danger: true });
  if (agent.live) menuItems.push({ label: 'Restart', onSelect: handleRestart });
  menuItems.push('separator');
  menuItems.push({ label: 'Edit...', onSelect: handleEdit });
  menuItems.push({ label: 'Delete...', onSelect: handleDelete, danger: true });

  return (
    <>
      <div
        className={styles.row}
        role="button"
        tabIndex={0}
        onClick={handleClick}
        onContextMenu={handleContextMenu}
        onKeyDown={handleKeyDown}
      >
        <span className={styles.dot} data-kind={dotKind} />
        <span className={styles.name}>{agent.name}</span>
        {agent.title && <span className={styles.title}>{agent.title}</span>}
        <span className={styles.profile}>{profileModel}</span>
        {showProject && <span className={styles.project}>{projectName(agent.project!)}</span>}
        {isNotResumable(agent) && <span className={styles.chip}>not resumable</span>}
        {restartNeeded && <span className={styles.chip}>restart to apply</span>}
      </div>
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} onClose={handleCloseMenu} items={menuItems} />
      )}
    </>
  );
};

export default AgentRow;
