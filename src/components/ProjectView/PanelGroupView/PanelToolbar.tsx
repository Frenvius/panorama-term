import type { NoteTab } from '~/usecase/util/noteTabs';
import type { RunPhase } from '~/domain/interfaces/run.interface';
import type { NoteTabActions } from '~/components/Canvas/NoteTabs';
import type { Tile } from '~/domain/interfaces/workspace.interface';
import type { AgentType } from '~/components/Terminal/AgentBar/parse';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import React from 'react';
import ReactDOM from 'react-dom';
import {
  X,
  Eye,
  Copy,
  Play,
  Save,
  Hammer,
  Search,
  Square,
  History,
  PenLine,
  ArrowUp,
  Loader2,
  RotateCw,
  ArrowDown,
  GitBranch,
  FolderOpen,
  GitCompare,
  ChevronDown,
  SquareTerminal
} from 'lucide-react';

import styles from './styles.module.scss';
import { useRun } from '~/usecase/hooks/useRun';
import NoteTabs from '~/components/Canvas/NoteTabs';
import { writeNote } from '~/adapter/notes/notes.client';
import { useBranches } from '~/usecase/hooks/useBranches';
import { revealPath } from '~/adapter/shell/shell.client';
import ContextMenu from '~/components/commons/ContextMenu';
import { isMarkdownFile } from '~/usecase/util/codeEditor';
import { useRepoState } from '~/usecase/hooks/useRepoState';
import { operationTag } from '~/usecase/util/repoOperation';
import { noteTheme, NOTE_PALETTE } from '~/usecase/util/note';
import BranchMenu from '~/components/Canvas/TileFrame/BranchMenu';
import { withBody, setFrontField } from '~/usecase/util/noteMeta';
import HistoryMenu from '~/components/Canvas/TileFrame/HistoryMenu';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { hasHistory, resumeCommand } from '~/usecase/util/agentHistory';
import { notifyClaude } from '~/components/commons/Notifications/bridge';
import { tabText, joinTabs, nameTabs, noteTabs } from '~/usecase/util/noteTabs';
import { isDirty, requestFind, requestSave, subscribeDirty, requestPreview } from '~/usecase/util/dirtyFiles';

export interface PanelToolbarHandlers {
  onToggleRaw: () => void;
  onRestart: (command?: string) => void;
  onPatchTile: (tileId: string, patch: Partial<Tile>) => void;
  onCwd: (tileId: string, cwd: string, branch?: string) => void;
  onOpenDiff: (root: string, filePath: string, opts?: { sideSplit?: boolean }) => void;
  onOpenFile: (root: string, filePath: string, opts?: { preview?: boolean; sideSplit?: boolean }) => void;
  onOpenRunOutput: (srcId: string, cwd: string, sessionId: string, cmd: string) => void;
}

export interface PanelToolbarProps {
  tile: Tile;
  wsId: string | null;
  noteTab: number;
  agentType: AgentType | null;
  diff: { a: number; r: number } | null;
  handlers: PanelToolbarHandlers;
}

const RUN_ICON = 13;
const NOTE_COLORS = NOTE_PALETTE;
const PHASE_LABEL: Record<RunPhase, string> = {
  starting: 'Starting',
  restarting: 'Restarting',
  stopping: 'Stopping',
  killing: 'Killing'
};

const commitCount = (n: number): string => (n > 99 ? '99+' : String(n));

const menuAtButton = (e: React.MouseEvent): { x: number; y: number } => {
  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
  return { x: r.left, y: r.bottom + 4 };
};

const toRelativePath = (root: string, fullPath: string): string => {
  const normRoot = root.replace(/\\/g, '/').replace(/\/+$/, '');
  const normFile = fullPath.replace(/\\/g, '/');
  if (normRoot && normFile.toLowerCase().startsWith(`${normRoot.toLowerCase()}/`)) {
    return normFile.slice(normRoot.length + 1);
  }
  return fullPath;
};

const toFullPath = (root: string, filePath: string): string => {
  if (/^(?:[A-Za-z]:[\\/]|\/)/.test(filePath)) return filePath;
  const normRoot = root.replace(/[\\/]+$/, '');
  const normRel = filePath.replace(/^[\\/]+/, '');
  return normRoot ? `${normRoot}/${normRel}` : filePath;
};

const PanelToolbar = (props: PanelToolbarProps) => {
  const { tile, wsId, noteTab, agentType, diff, handlers } = props;
  const isTerm = tile.type === 'term';
  const isNote = tile.type === 'note';
  const isCode = tile.type === 'code';
  const isEditor = tile.type === 'editor';
  const runView = Boolean(tile.runCwd);

  const [restarting, setRestarting] = React.useState(false);
  const [runMenu, setRunMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [buildMenu, setBuildMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [historyMenu, setHistoryMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [branchAnchor, setBranchAnchor] = React.useState<{ x: number; y: number } | null>(null);
  const [editorDirty, setEditorDirty] = React.useState(() => (tile.filePath ? isDirty(tile.filePath) : false));

  React.useEffect(() => {
    if (!isEditor || !tile.filePath) {
      setEditorDirty(false);
      return;
    }
    const targetPath = tile.filePath;
    setEditorDirty(isDirty(targetPath));
    return subscribeDirty(() => setEditorDirty(isDirty(targetPath)));
  }, [isEditor, tile.filePath]);

  React.useEffect(() => {
    if (!restarting) return;
    const timer = setTimeout(() => setRestarting(false), 700);
    return () => clearTimeout(timer);
  }, [restarting]);

  const termCwd = isTerm ? tile.cwd : undefined;
  const repo = useRepoState(termCwd);
  const branches = useBranches(termCwd, branchAnchor !== null, repo);
  const repoLabel = repo ? (repo.branch ?? repo.head ?? undefined) : undefined;
  const branchLabel = repo ? repoLabel : tile.branch;

  React.useEffect(() => {
    if (!isTerm || !repo || !tile.cwd || repoLabel === tile.branch) return;
    handlers.onCwd(tile.id, tile.cwd, repoLabel);
  }, [isTerm, repo, repoLabel, tile.id, tile.cwd, tile.branch, handlers]);

  const runCwd = isTerm ? (tile.runCwd ?? tile.cwd) : undefined;
  const runTile = isTerm ? (runView ? tile.ptySessionId?.replace(/^(run|build):/, '') : tile.id) : undefined;
  const viewKind = runView && tile.ptySessionId?.startsWith('build:') ? ('build' as const) : ('run' as const);
  const run = useRun(runCwd, runTile, viewKind);
  const build = useRun(runView ? undefined : runCwd, runView ? undefined : tile.id, 'build');

  const running = run.status.state === 'running';
  const runFailed = run.status.state === 'exited' && (run.status.exitCode ?? 0) !== 0 && !run.status.stoppedByUser;
  const building = build.status.state === 'running';
  const buildOk = build.status.state === 'exited' && (build.status.exitCode ?? 0) === 0;
  const buildFailed = build.status.state === 'exited' && (build.status.exitCode ?? 0) !== 0 && !build.status.stoppedByUser;
  const showRunBtn = isTerm && !runView && Boolean(tile.cwd) && (run.commands.length > 0 || run.status.state !== 'none');
  const showBuildBtn = isTerm && !runView && Boolean(tile.cwd) && (build.commands.length > 0 || build.status.state !== 'none');

  React.useEffect(() => {
    if (!run.crashed) return;
    run.clearCrashed();
    if (!runView) {
      notifyClaude(
        tile.id,
        'generic',
        `${run.crashed.cmd ?? 'run'} exited with code ${run.crashed.exitCode ?? '?'}`,
        'Run crashed'
      );
    }
  }, [run, runView, tile.id]);

  React.useEffect(() => {
    if (!build.crashed) return;
    build.clearCrashed();
    notifyClaude(
      tile.id,
      'generic',
      `${build.crashed.cmd ?? 'build'} exited with code ${build.crashed.exitCode ?? '?'}`,
      'Build failed'
    );
  }, [build, tile.id]);

  const buildStop = build.stop;
  React.useEffect(() => {
    if (!buildOk) return;
    const timer = setTimeout(() => buildStop(), 3000);
    return () => clearTimeout(timer);
  }, [buildOk, buildStop]);

  const runBusy = running || run.phase !== null;
  const buildBusy = building || build.phase !== null;
  const runKillable = run.phase === 'stopping' || run.phase === 'killing';
  const runCmd = run.status.cmd ?? run.defaultCmd;
  const buildCmd = build.status.cmd ?? build.defaultCmd;
  const runLine = run.lastLine ? ` - ${run.lastLine}` : '';
  const runTip = run.phase
    ? `${PHASE_LABEL[run.phase]} '${runCmd}'${runLine}`
    : running
      ? `Rerun '${runCmd}'${runLine}`
      : runFailed
        ? `'${runCmd}' exited with code ${run.status.exitCode}`
        : run.defaultCmd
          ? `Run '${run.defaultCmd}'`
          : 'Run';
  const runStopTip =
    run.phase === 'stopping' ? 'Kill process now' : run.phase === 'killing' ? 'Killing process tree' : `Stop '${runCmd}'`;
  const buildTip = build.phase
    ? `${PHASE_LABEL[build.phase]} '${buildCmd}'`
    : building
      ? `Rebuild '${buildCmd}'`
      : buildFailed
        ? `'${buildCmd}' failed with code ${build.status.exitCode}`
        : buildOk
          ? `'${buildCmd}' finished`
          : build.defaultCmd
            ? `Build '${build.defaultCmd}'`
            : 'Build';

  const runBtnClass = runBusy
    ? `${styles.toolBtn} ${styles.runBtn} ${styles.busyFill}${run.phase ? ` ${styles.pending}` : ''}`
    : `${styles.toolBtn} ${styles.runBtn} ${styles.runIdle}`;
  const buildBtnClass = buildBusy
    ? `${styles.toolBtn} ${styles.runBtn} ${styles.busyFill}${build.phase ? ` ${styles.pending}` : ''}`
    : `${styles.toolBtn} ${styles.runBtn}`;

  const runIcon =
    run.phase === 'restarting' ? (
      <RotateCw size={RUN_ICON} strokeWidth={2} className={styles.spin} />
    ) : run.phase ? (
      <Loader2 size={RUN_ICON} strokeWidth={2} className={styles.spin} />
    ) : running ? (
      <RotateCw size={RUN_ICON} strokeWidth={2} />
    ) : (
      <Play size={RUN_ICON} strokeWidth={2} />
    );

  const openRunOutput = () => {
    const sid = run.status.sessionId;
    if (sid && tile.cwd) handlers.onOpenRunOutput(tile.id, tile.cwd, sid, run.status.cmd ?? run.defaultCmd ?? 'run');
  };

  const openBuildOutput = () => {
    const sid = build.status.sessionId;
    if (sid && tile.cwd) handlers.onOpenRunOutput(tile.id, tile.cwd, sid, build.status.cmd ?? build.defaultCmd ?? 'build');
  };

  const onRunClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (running) run.restart();
    else if (run.status.state === 'exited') run.start(run.status.cmd);
    else run.start();
  };

  const onRunStop = (e: React.MouseEvent) => {
    e.stopPropagation();
    run.stop(runKillable);
  };

  const onRunCaret = (e: React.MouseEvent) => {
    e.stopPropagation();
    setRunMenu(menuAtButton(e));
  };

  const stopRunning = () => run.stop();
  const stopBuilding = () => build.stop(true);

  const onBuildClick = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (building) build.restart();
    else build.start();
  };

  const onBuildStop = (e: React.MouseEvent) => {
    e.stopPropagation();
    stopBuilding();
  };

  const onBuildCaret = (e: React.MouseEvent) => {
    e.stopPropagation();
    setBuildMenu(menuAtButton(e));
  };

  const closeRunMenu = () => setRunMenu(null);
  const closeBuildMenu = () => setBuildMenu(null);
  const closeHistory = () => setHistoryMenu(null);
  const closeBranches = () => setBranchAnchor(null);

  const openBranches = (e: React.MouseEvent) => {
    e.stopPropagation();
    setBranchAnchor(menuAtButton(e));
  };

  const historyAgent = hasHistory(agentType) && tile.cwd ? agentType : null;
  const openHistory = (e: React.MouseEvent) => {
    e.stopPropagation();
    setHistoryMenu(menuAtButton(e));
  };

  const resumeConversation = (sessionId: string) => {
    const command = historyAgent && resumeCommand(historyAgent, sessionId);
    if (command) {
      setRestarting(true);
      handlers.onRestart(command);
    }
  };

  const triggerRestart = () => {
    setRestarting(true);
    handlers.onRestart();
  };

  const copyTargetPath = () => {
    const p = isTerm ? (tile.runCwd ?? tile.cwd) : (tile.filePath ?? tile.cwd);
    if (p) writeClipboard(p);
  };

  const revealTargetPath = () => {
    const p = isTerm ? (tile.runCwd ?? tile.cwd) : tile.filePath ? toFullPath(tile.cwd ?? '', tile.filePath) : tile.cwd;
    if (p) revealPath(p);
  };

  const saveEditorFile = () => {
    if (tile.filePath) void requestSave(tile.filePath);
  };

  const openEditorDiff = () => {
    if (!tile.cwd || !tile.filePath) return;
    handlers.onOpenDiff(tile.cwd, toRelativePath(tile.cwd, tile.filePath));
  };

  const editDiffFile = () => {
    const root = tile.cwd ?? '';
    const filePath = tile.filePath ?? '';
    if (!filePath) return;
    handlers.onOpenFile(root, toFullPath(root, filePath));
  };

  const sections = isNote ? noteTabs(tile.content) : [];
  const tint = isNote ? noteTheme(tile.color) : null;

  const saveTabs = (next: NoteTab[], activeIdx: number) => {
    const named = nameTabs(next);
    const current = named.length > 1 ? named[activeIdx].name : '';
    const full = setFrontField(withBody(tile.content || '', joinTabs(named)), 'tab', current);
    handlers.onPatchTile(tile.id, { content: full });
    if (wsId) void writeNote(wsId, tile.id, full).catch(() => {});
  };

  const noteTabActions: NoteTabActions = {
    select: (i) => {
      if (i !== noteTab) saveTabs(sections, i);
    },
    add: () => saveTabs([...sections, { name: '', text: '' }], sections.length),
    rename: (i, name) => {
      if (sections.some((s, j) => j !== i && s.name === name)) return;
      saveTabs(
        sections.map((s, j) => (j === i ? { ...s, name } : s)),
        noteTab
      );
    },
    close: (i) => {
      const rest = sections.filter((_, j) => j !== i);
      const next = i < noteTab ? noteTab - 1 : Math.min(noteTab, rest.length - 1);
      saveTabs(rest.length === 1 ? [{ name: '', text: rest[0].text }] : rest, next);
    }
  };

  const copyNoteBody = () => {
    writeClipboard(tabText(tile.content, noteTab));
  };

  const selectNoteColor = (colorHex: string) => () => {
    handlers.onPatchTile(tile.id, { color: colorHex });
  };

  const buildMenuItems: ContextMenuEntry[] = building
    ? [
        { label: 'Open output', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: openBuildOutput },
        'separator',
        { label: 'Stop build', icon: <X size={15} strokeWidth={1.75} />, danger: true, onSelect: stopBuilding }
      ]
    : [
        ...(build.status.sessionId
          ? ([
              { label: 'Open output', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: openBuildOutput },
              'separator'
            ] as ContextMenuEntry[])
          : []),
        ...build.commands.map((cmd) => ({
          label: cmd === build.defaultCmd ? `${cmd}  (default)` : cmd,
          icon: <Hammer size={15} strokeWidth={1.75} />,
          onSelect: () => build.start(cmd)
        }))
      ];

  const runMenuItems: ContextMenuEntry[] =
    run.status.state === 'none'
      ? run.commands.map((cmd) => ({
          label: cmd === run.defaultCmd ? `${cmd}  (default)` : cmd,
          icon: <Play size={15} strokeWidth={1.75} />,
          onSelect: () => run.start(cmd)
        }))
      : running
        ? [
            { label: 'Open output', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: openRunOutput },
            { label: 'Restart', icon: <RotateCw size={15} strokeWidth={1.75} />, onSelect: run.restart },
            'separator',
            { label: 'Stop', icon: <X size={15} strokeWidth={1.75} />, danger: true, onSelect: stopRunning }
          ]
        : [
            { label: 'Open output', icon: <SquareTerminal size={15} strokeWidth={1.75} />, onSelect: openRunOutput },
            { label: 'Run again', icon: <Play size={15} strokeWidth={1.75} />, onSelect: () => run.start(run.status.cmd) },
            'separator',
            { label: 'Clear status', icon: <X size={15} strokeWidth={1.75} />, onSelect: () => run.stop() }
          ];

  const noteTabsStyle: React.CSSProperties = {
    position: 'relative',
    flex: 1,
    minWidth: 120
  };

  const toolbarClass = tint ? `${styles.toolbar} ${styles.noteToolbar}` : styles.toolbar;
  const toolbarStyle = tint ? { ['--note-body' as string]: tint.body, ['--note-text' as string]: tint.text } : undefined;

  return (
    <div className={toolbarClass} style={toolbarStyle}>
      {isTerm && (
        <>
          {(tile.runCwd || tile.cwd) && (
            <div className={styles.pathGroup}>
              <button
                type="button"
                className={styles.pathChip}
                onClick={copyTargetPath}
                title={`Click to copy: ${tile.runCwd ?? tile.cwd}`}
              >
                <span className={styles.pathText}>{tile.runCwd ?? tile.cwd}</span>
              </button>
              <button
                type="button"
                className={styles.toolBtn}
                onClick={revealTargetPath}
                aria-label="Reveal in explorer"
                data-tooltip="Reveal in explorer"
              >
                <FolderOpen size={13} strokeWidth={1.75} />
              </button>
            </div>
          )}

          {branchLabel && (
            <button
              type="button"
              className={styles.branchChip}
              onClick={openBranches}
              data-operation={Boolean(repo?.state) || undefined}
            >
              <GitBranch size={11} strokeWidth={2} />
              <span className={styles.branchText}>{branchLabel}</span>
              {repo?.state && <span className={styles.operation}>{operationTag(repo)}</span>}
              {repo && repo.ahead > 0 && (
                <span className={styles.ahead} data-tooltip={`${repo.ahead} to push`}>
                  <ArrowUp size={9} strokeWidth={2.5} />
                  {commitCount(repo.ahead)}
                </span>
              )}
              {repo && repo.behind > 0 && (
                <span className={styles.behind} data-tooltip={`${repo.behind} to pull`}>
                  <ArrowDown size={9} strokeWidth={2.5} />
                  {commitCount(repo.behind)}
                </span>
              )}
              <ChevronDown size={10} strokeWidth={2} />
            </button>
          )}

          <span className={styles.toolbarSpacer} />

          {!runView && diff && (
            <span className={styles.diffStat}>
              <span className={styles.diffAdd}>+{diff.a}</span>
              <span className={styles.diffDel}>-{diff.r}</span>
            </span>
          )}

          {showBuildBtn && (
            <div className={styles.runGroup}>
              <button
                type="button"
                className={buildBtnClass}
                onClick={onBuildClick}
                disabled={build.phase !== null}
                aria-label={building ? 'Rebuild project' : 'Build project'}
                data-tooltip={buildTip}
              >
                {build.phase ? (
                  <Loader2 size={RUN_ICON} strokeWidth={2} className={styles.spin} />
                ) : (
                  <Hammer size={RUN_ICON} strokeWidth={2} />
                )}
                {buildOk && <span className={`${styles.statusBadge} ${styles.badgeOk}`} />}
                {buildFailed && <span className={`${styles.statusBadge} ${styles.badgeErr}`} />}
              </button>
              {buildBusy && (
                <button
                  type="button"
                  className={`${styles.toolBtn} ${styles.runBtn} ${styles.stopFill}`}
                  onClick={onBuildStop}
                  disabled={build.phase === 'killing'}
                  aria-label="Stop build"
                  data-tooltip={build.phase === 'killing' ? 'Killing process tree' : `Stop '${buildCmd}'`}
                >
                  <Square size={RUN_ICON} strokeWidth={2} fill="currentColor" />
                </button>
              )}
              {(building || buildFailed || build.commands.length > 1) && (
                <button type="button" className={styles.toolBtn} onClick={onBuildCaret} aria-label="Build menu">
                  <ChevronDown size={11} strokeWidth={2} />
                </button>
              )}
            </div>
          )}

          {showRunBtn && (
            <div className={styles.runGroup}>
              <button
                type="button"
                className={runBtnClass}
                onClick={onRunClick}
                disabled={run.phase !== null}
                aria-label={running ? 'Rerun project' : 'Run project'}
                data-tooltip={runTip}
              >
                {runIcon}
                {runFailed && !run.phase && <span className={`${styles.statusBadge} ${styles.badgeErr}`} />}
              </button>
              {runBusy && (
                <button
                  type="button"
                  className={`${styles.toolBtn} ${styles.runBtn} ${styles.stopFill}`}
                  onClick={onRunStop}
                  disabled={run.phase === 'killing'}
                  aria-label={runKillable ? 'Kill process' : 'Stop run'}
                  data-tooltip={runStopTip}
                >
                  {runKillable ? (
                    <X size={RUN_ICON} strokeWidth={2.5} />
                  ) : (
                    <Square size={RUN_ICON} strokeWidth={2} fill="currentColor" />
                  )}
                </button>
              )}
              {(run.status.state !== 'none' || run.commands.length > 1) && (
                <button type="button" className={styles.toolBtn} onClick={onRunCaret} aria-label="Run menu">
                  <ChevronDown size={11} strokeWidth={2} />
                </button>
              )}
            </div>
          )}

          {runView && (
            <span className={running ? styles.runState : `${styles.runState} ${styles.runStateErr}`}>
              {running ? 'running' : run.status.state === 'exited' ? `exit ${run.status.exitCode ?? '?'}` : ''}
            </span>
          )}

          {!runView && historyAgent && (
            <button
              type="button"
              className={historyMenu ? `${styles.toolBtn} ${styles.toolActive}` : styles.toolBtn}
              onClick={openHistory}
              aria-label="Conversation history"
              data-tooltip="Conversations"
            >
              <History size={13} strokeWidth={2} />
            </button>
          )}

          {!runView && (
            <button
              type="button"
              className={styles.toolBtn}
              onClick={triggerRestart}
              aria-label="Restart terminal"
              data-tooltip={restarting ? 'Restarting' : 'Restart shell'}
            >
              <RotateCw size={13} strokeWidth={2} className={restarting ? styles.spin : undefined} />
            </button>
          )}
        </>
      )}

      {isEditor && (
        <>
          <div className={styles.pathGroup}>
            <button
              type="button"
              className={styles.pathChip}
              onClick={copyTargetPath}
              title={tile.filePath ? `Click to copy: ${tile.filePath}` : undefined}
            >
              <span className={styles.pathText}>{tile.filePath ?? 'Untitled'}</span>
            </button>
            {editorDirty && <span className={styles.dirtyBadge}>Modified</span>}
            {tile.filePath && (
              <button
                type="button"
                className={styles.toolBtn}
                onClick={revealTargetPath}
                aria-label="Reveal in explorer"
                data-tooltip="Reveal in explorer"
              >
                <FolderOpen size={13} strokeWidth={1.75} />
              </button>
            )}
          </div>

          <span className={styles.toolbarSpacer} />

          <button
            type="button"
            className={styles.toolActionBtn}
            onClick={saveEditorFile}
            disabled={!editorDirty}
            aria-label="Save file"
            data-tooltip="Save file"
          >
            <Save size={12} strokeWidth={2} />
            <span>Save</span>
          </button>

          <button
            type="button"
            className={styles.toolActionBtn}
            onClick={requestFind}
            aria-label="Find in file"
            data-tooltip="Find in file"
          >
            <Search size={12} strokeWidth={2} />
            <span>Find</span>
          </button>

          {isMarkdownFile(tile.filePath ?? '') && (
            <button
              type="button"
              className={styles.toolActionBtn}
              onClick={requestPreview}
              aria-label="Toggle markdown preview"
              data-tooltip="Toggle markdown preview"
            >
              <Eye size={12} strokeWidth={2} />
              <span>Preview</span>
            </button>
          )}

          {tile.cwd && tile.filePath && (
            <button
              type="button"
              className={styles.toolActionBtn}
              onClick={openEditorDiff}
              aria-label="Open diff"
              data-tooltip="Open diff"
            >
              <GitCompare size={12} strokeWidth={2} />
              <span>Open diff</span>
            </button>
          )}
        </>
      )}

      {isCode && (
        <>
          <div className={styles.pathGroup}>
            <button
              type="button"
              className={styles.pathChip}
              onClick={copyTargetPath}
              title={tile.filePath ? `Click to copy: ${tile.filePath}` : undefined}
            >
              <span className={styles.pathText}>{tile.filePath ?? 'Diff'}</span>
            </button>
            {tile.filePath && (
              <button
                type="button"
                className={styles.toolBtn}
                onClick={revealTargetPath}
                aria-label="Reveal in explorer"
                data-tooltip="Reveal in explorer"
              >
                <FolderOpen size={13} strokeWidth={1.75} />
              </button>
            )}
          </div>

          <span className={styles.toolbarSpacer} />

          {tile.filePath && (
            <button
              type="button"
              className={styles.toolActionBtn}
              onClick={editDiffFile}
              aria-label="Edit file"
              data-tooltip="Edit file"
            >
              <PenLine size={12} strokeWidth={2} />
              <span>Edit file</span>
            </button>
          )}
        </>
      )}

      {isNote && (
        <>
          <NoteTabs
            current={noteTab}
            raised={true}
            variant="panel"
            actions={noteTabActions}
            style={noteTabsStyle}
            tabs={sections.map((s) => ({ name: s.name, empty: !s.text.trim() }))}
          />

          <div className={styles.colorSwatches}>
            {NOTE_COLORS.map((c) => {
              const activeColor = (tile.color || NOTE_COLORS[0].body).toLowerCase() === c.body.toLowerCase();
              return (
                <button
                  key={c.body}
                  type="button"
                  className={activeColor ? `${styles.colorDot} ${styles.colorDotActive}` : styles.colorDot}
                  style={{ background: c.body }}
                  onClick={selectNoteColor(c.body)}
                  aria-label={`Note color ${c.body}`}
                />
              );
            })}
          </div>

          <button
            type="button"
            className={tile.renderOnly ? `${styles.toolBtn} ${styles.rawOn}` : styles.toolBtn}
            onClick={handlers.onToggleRaw}
            aria-label={tile.renderOnly ? 'Show markdown on edit' : 'Rendered only'}
            data-tooltip={tile.renderOnly ? 'Rendered' : 'Live edit'}
          >
            <span className={styles.rawGlyph}>M</span>
          </button>

          <button type="button" className={styles.toolBtn} onClick={copyNoteBody} aria-label="Copy note" data-tooltip="Copy note">
            <Copy size={13} strokeWidth={2} />
          </button>
        </>
      )}

      {runMenu && <ContextMenu x={runMenu.x} y={runMenu.y} items={runMenuItems} onClose={closeRunMenu} />}
      {buildMenu && <ContextMenu x={buildMenu.x} y={buildMenu.y} items={buildMenuItems} onClose={closeBuildMenu} />}
      {historyMenu && historyAgent && tile.cwd && (
        <HistoryMenu
          x={historyMenu.x}
          y={historyMenu.y}
          cwd={tile.cwd}
          agent={historyAgent}
          tileId={tile.ptySessionId ?? tile.id}
          onPick={resumeConversation}
          onClose={closeHistory}
        />
      )}
      {branchAnchor &&
        tile.cwd &&
        ReactDOM.createPortal(
          <BranchMenu
            k={1}
            cwd={tile.cwd}
            anchor={branchAnchor}
            zIndex={1000}
            snapshot={branches.snapshot}
            loading={branches.loading}
            error={branches.error}
            onClose={closeBranches}
            onSnapshot={branches.setSnapshot}
            onError={branches.setError}
          />,
          document.body
        )}
    </div>
  );
};

export default PanelToolbar;
