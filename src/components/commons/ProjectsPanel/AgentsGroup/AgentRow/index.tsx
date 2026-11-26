import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { PersistentAgent } from '~/domain/interfaces/agents.interface';

import React from 'react';
import { Clock, Hourglass, AlarmClock } from 'lucide-react';

import styles from './styles.module.scss';
import { showToast } from '~/usecase/util/toasts';
import { agentActions } from '~/usecase/hooks/useAgents';
import ContextMenu from '~/components/commons/ContextMenu';
import { projectName } from '~/usecase/util/projectLayout';
import { requestShowAgent } from '~/usecase/util/agentRequest';
import { startAgent, stopAgent } from '~/adapter/agents/agents.client';
import ConfirmDialog from '~/components/commons/Settings/ConfirmDialog';
import { startedByLabel, waitTooltip } from '~/usecase/util/agentWaits';
import AgentWaits from '~/components/commons/ProjectsPanel/AgentsGroup/AgentWaits';
import { statusToDotKind, isNotResumable, needsForceStop } from '~/usecase/util/agents';
import AgentRoutines from '~/components/commons/ProjectsPanel/AgentsGroup/AgentRoutines';
import AgentMessages from '~/components/commons/ProjectsPanel/AgentsGroup/AgentMessages';

const SHOW_WHEN_LIVE_MS = 120_000;

interface AgentRowProps {
  agent: PersistentAgent;
  activeProject: string | null;
  restartNeeded: boolean;
  onEdit: () => void;
  onDelete: () => void;
}

const reportError = (title: string) => (err: unknown) => {
  showToast({ kind: 'error', message: title, detail: err instanceof Error ? err.message : String(err) });
  agentActions.refresh();
};

const AgentRow = ({ agent, activeProject, restartNeeded, onEdit, onDelete }: AgentRowProps) => {
  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [showWhenLive, setShowWhenLive] = React.useState(false);
  const [confirmRestart, setConfirmRestart] = React.useState(false);
  const [showMessages, setShowMessages] = React.useState(false);
  const [messagesAnchor, setMessagesAnchor] = React.useState<DOMRect | null>(null);
  const [waitsAnchor, setWaitsAnchor] = React.useState<DOMRect | null>(null);
  const [routinesAnchor, setRoutinesAnchor] = React.useState<DOMRect | null>(null);
  const rowRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    if (!showWhenLive) return undefined;
    const expire = setTimeout(() => setShowWhenLive(false), SHOW_WHEN_LIVE_MS);
    return () => clearTimeout(expire);
  }, [showWhenLive]);

  React.useEffect(() => {
    if (!showWhenLive || !agent.live) return;
    setShowWhenLive(false);
    requestShowAgent({ tileId: agent.tileId, cwd: agent.cwd });
  }, [showWhenLive, agent.live, agent.tileId, agent.cwd]);

  const profileModel = agent.model ? `${agent.profile}:${agent.model}` : agent.profile;
  const showProject = agent.project && agent.project !== activeProject;
  const dotKind = statusToDotKind(agent.status);
  const forceOnly = needsForceStop(agent);
  const exitDetail = agent.lastExit?.detail ?? agent.lastExit?.reason;
  const waits = agent.waits ?? [];

  const handleClick = () => {
    if (agent.live) {
      requestShowAgent({ tileId: agent.tileId, cwd: agent.cwd });
      return;
    }
    setShowWhenLive(true);
    startAgent(agent.id).then(() => agentActions.refresh()).catch((err) => {
      setShowWhenLive(false);
      reportError(`Could not start ${agent.name}`)(err);
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
    startAgent(agent.id).then(() => agentActions.refresh()).catch(reportError(`Could not start ${agent.name}`));
  };

  const handleStop = () => {
    setMenu(null);
    stopAgent(agent.id, forceOnly).then(() => agentActions.refresh()).catch(reportError(`Could not stop ${agent.name}`));
  };

  const handleRestartRequest = () => {
    setMenu(null);
    setConfirmRestart(true);
  };

  const handleRestartConfirm = () => {
    setConfirmRestart(false);
    stopAgent(agent.id, true)
      .then(() => startAgent(agent.id, true))
      .then(() => agentActions.refresh())
      .catch(reportError(`Could not restart ${agent.name}`));
  };

  const handleRestartCancel = () => setConfirmRestart(false);

  const handleEdit = () => {
    setMenu(null);
    onEdit();
  };

  const handleDelete = () => {
    setMenu(null);
    onDelete();
  };

  const handleOpenMessages = () => {
    setMenu(null);
    if (rowRef.current) setMessagesAnchor(rowRef.current.getBoundingClientRect());
    setShowMessages(true);
  };

  const handleCloseMessages = () => setShowMessages(false);

  const handleOpenWaits = () => {
    setMenu(null);
    if (rowRef.current) setWaitsAnchor(rowRef.current.getBoundingClientRect());
  };

  const handleCloseWaits = () => setWaitsAnchor(null);

  const handleOpenRoutines = () => {
    setMenu(null);
    if (rowRef.current) setRoutinesAnchor(rowRef.current.getBoundingClientRect());
  };

  const handleCloseRoutines = () => setRoutinesAnchor(null);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleClick();
    }
  };

  const menuItems: ContextMenuEntry[] = [];
  if (!agent.live) menuItems.push({ label: 'Start', onSelect: handleStart });
  if (agent.live) menuItems.push({ label: forceOnly ? 'Force stop' : 'Stop', onSelect: handleStop, danger: forceOnly });
  if (agent.live) menuItems.push({ label: 'Restart (new session)...', onSelect: handleRestartRequest });
  menuItems.push('separator');
  menuItems.push({ label: 'Messages...', onSelect: handleOpenMessages });
  menuItems.push({ label: 'Waits...', onSelect: handleOpenWaits });
  menuItems.push({ label: 'Routines...', onSelect: handleOpenRoutines });
  menuItems.push('separator');
  menuItems.push({ label: 'Edit...', onSelect: handleEdit });
  menuItems.push({ label: 'Delete...', onSelect: handleDelete, danger: true });

  return (
    <>
      <div
        ref={rowRef}
        className={styles.row}
        role="button"
        tabIndex={0}
        title={agent.status === 'crashed' ? exitDetail : undefined}
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
        {agent.startedBy && (
          <span className={styles.chip} title={agent.startedBy}>{startedByLabel(agent.startedBy)}</span>
        )}
        {agent.awaitingReplies > 0 && (
          <span className={styles.awaitIcon} title={`awaiting ${agent.awaitingReplies} ${agent.awaitingReplies === 1 ? 'reply' : 'replies'}`}>
            <Clock size={10} strokeWidth={2} />
          </span>
        )}
        {(agent.routines?.enabled ?? 0) > 0 && (
          <span className={styles.waitIcon} title={agent.routines?.nextRunLocal ? `next routine run ${agent.routines.nextRunLocal}` : 'routines enabled'}>
            <AlarmClock size={10} strokeWidth={2} />
          </span>
        )}
        {waits.length > 0 && (
          <span className={styles.waitIcon} title={waitTooltip(waits, Date.now())}>
            <Hourglass size={10} strokeWidth={2} />
          </span>
        )}
        {agent.queued > 0 && (
          <span className={styles.queuedBadge}>{agent.queued} queued</span>
        )}
      </div>
      {menu && (
        <ContextMenu x={menu.x} y={menu.y} onClose={handleCloseMenu} items={menuItems} />
      )}
      {confirmRestart && (
        <ConfirmDialog
          title="Restart agent"
          message={`Restart ${agent.name} in a new session? The current conversation is not resumed.`}
          confirmLabel="Restart"
          danger
          onConfirm={handleRestartConfirm}
          onCancel={handleRestartCancel}
        />
      )}
      {routinesAnchor && (
        <AgentRoutines agent={agent} anchorRect={routinesAnchor} onClose={handleCloseRoutines} />
      )}
      {waitsAnchor && (
        <AgentWaits agent={agent} anchorRect={waitsAnchor} onClose={handleCloseWaits} />
      )}
      {showMessages && messagesAnchor && (
        <AgentMessages agent={agent} anchorRect={messagesAnchor} onClose={handleCloseMessages} />
      )}
    </>
  );
};

export default AgentRow;
