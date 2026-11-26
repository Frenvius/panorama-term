import type { FeedLine } from '~/usecase/util/codexFeed';
import type { BgTask, TasksDetail } from '~/usecase/util/bgTasks';

import React from 'react';
import { Bot, Radio, Trash2, ChevronDown, ChevronRight, SquareTerminal } from 'lucide-react';

import { formatElapsed } from '~/usecase/util/elapsed';
import { useFileTail } from '~/usecase/hooks/useFileTail';
import { useCodexFeed } from '~/usecase/hooks/useCodexFeed';
import { CodexLogo, KimiLogo, OpenCodeLogo, AntigravityLogo } from '~/components/commons/AgentIcons';
import { useBgTasks, useTasksDetail, clearFinished, cliOf } from '~/usecase/util/bgTasks';

import styles from './styles.module.scss';

const STATUS_LABEL: Record<string, string> = { completed: 'done', failed: 'failed', killed: 'stopped' };
const FEED_PREFIX: Record<string, string> = {
  command: '$',
  tool: '⚙',
  edit: '✎',
  reasoning: '…',
  running: '▸',
  done: '✓',
  message: ''
};
const ICON = 13;

const lastLine = (text: string) => text.trimEnd().split('\n').at(-1) ?? '';

const TaskIcon = ({ task, cli }: { task: BgTask; cli?: string }) => {
  if (task.kind === 'agent') return <Bot size={ICON} />;
  if (task.monitor) return <Radio size={ICON} />;
  if (cli === 'codex') return <CodexLogo size={ICON} />;
  if (cli === 'kimi') return <KimiLogo size={ICON} />;
  if (cli === 'opencode') return <OpenCodeLogo size={ICON} />;
  if (cli === 'agy' || cli === 'antigravity') return <AntigravityLogo size={ICON} />;
  return <SquareTerminal size={ICON} />;
};

const Feed = ({ lines, detail }: { lines: FeedLine[]; detail: TasksDetail }) => {
  const shown = detail === 'full' ? lines : lines.filter((l) => l.kind !== 'reasoning');
  const ref = React.useRef<HTMLDivElement>(null);

  React.useLayoutEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [shown.length]);

  if (!shown.length) return <p className={styles.note}>Waiting for Codex to start working.</p>;
  return (
    <div ref={ref} className={styles.feed}>
      {shown.map((l, i) => (
        <div key={i} className={styles.feedLine} data-kind={l.kind} data-failed={l.failed || undefined}>
          {FEED_PREFIX[l.kind] && <span className={styles.feedMark}>{FEED_PREFIX[l.kind]}</span>}
          <span>{l.text}</span>
        </div>
      ))}
    </div>
  );
};

const Output = ({ text, error }: { text: string; error: string | null }) => {
  const ref = React.useRef<HTMLPreElement>(null);

  React.useLayoutEffect(() => {
    if (ref.current) ref.current.scrollTop = ref.current.scrollHeight;
  }, [text]);

  return (
    <pre ref={ref} className={styles.output}>
      {error ?? text}
    </pre>
  );
};

interface TaskItemProps {
  task: BgTask;
  now: number;
  open: boolean;
  // False while the tile is off screen or behind a fullscreen tile: nothing is read then.
  visible: boolean;
  detail: TasksDetail;
  onToggle: () => void;
}

const TaskItem = ({ task, now, open, visible, detail, onToggle }: TaskItemProps) => {
  const agent = task.kind === 'agent';
  const cli = agent ? undefined : cliOf(task.command);
  const codex = cli === 'codex';
  const running = !task.endedAt;
  // The summary line needs the live source even while the task is collapsed.
  const summarize = detail !== 'compact' && running;
  const read = visible && (open || summarize);
  const feed = useCodexFeed(task, codex && read);
  const { text, error } = useFileTail(!agent && !codex && read ? task.outputFile : undefined, { live: running });

  const summary = agent
    ? (task.label ?? task.activity?.at(-1))
    : codex
      ? feed.lines.filter((l) => l.kind !== 'reasoning').at(-1)?.text
      : lastLine(text);

  return (
    <div className={styles.task} data-ended={!running}>
      <button type="button" className={styles.taskHead} onClick={onToggle} aria-expanded={open} title={task.command}>
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        <span className={styles.icon}>
          <TaskIcon task={task} cli={cli} />
        </span>
        {running && <span className={styles.dot} />}
        <span className={styles.title}>{task.description || task.command}</span>
        {task.status && (
          <span className={styles.status} data-status={task.status}>
            {STATUS_LABEL[task.status] ?? task.status}
          </span>
        )}
        <span className={styles.elapsed}>{formatElapsed((task.endedAt ?? now) - task.startedAt)}</span>
      </button>
      {!open && summarize && summary && <div className={styles.summary}>{summary}</div>}
      {open && (
        <div className={styles.taskBody}>
          {agent ? (
            <>
              <div className={styles.meta}>
                {[task.command, task.model, task.tokens ? `${Math.round(task.tokens / 1000)}k tokens` : '']
                  .filter(Boolean)
                  .join(' · ')}
              </div>
              {task.label && <p className={styles.label}>{task.label}</p>}
              {task.activity?.length ? (
                <div className={styles.feed}>
                  {task.activity.map((line, i) => (
                    <div key={i} className={styles.feedLine} data-kind="tool">
                      <span>{line}</span>
                    </div>
                  ))}
                </div>
              ) : (
                !task.label && <p className={styles.note}>{running ? 'Starting.' : 'No activity recorded.'}</p>
              )}
            </>
          ) : (
            <>
              <pre className={styles.command}>$ {task.command}</pre>
              {codex ? (
                feed.found || running ? (
                  <Feed lines={feed.lines} detail={detail} />
                ) : (
                  <p className={styles.note}>Codex session log not found.</p>
                )
              ) : task.outputFile ? (
                <Output text={text} error={error} />
              ) : (
                <p className={styles.note}>
                  {running
                    ? 'Runs in the foreground; its output reaches the agent when it ends.'
                    : 'Output not kept for this task.'}
                </p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
};

const TasksTile = ({ sourceId, visible }: { sourceId: string; visible: boolean }) => {
  const tasks = useBgTasks(sourceId);
  const detail = useTasksDetail();
  const [toggled, setToggled] = React.useState<Set<string>>(() => new Set());
  const [finishedOpen, setFinishedOpen] = React.useState(true);
  const [now, setNow] = React.useState(Date.now);
  const running = tasks.filter((t) => !t.endedAt);
  const finished = tasks.filter((t) => t.endedAt);
  const anyRunning = running.length > 0;

  React.useEffect(() => {
    if (!anyRunning || !visible) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [anyRunning, visible]);

  // Full detail opens every running task, normal the newest; clicking flips whatever the default was.
  const openByDefault = (t: BgTask) => !t.endedAt && (detail === 'full' || (detail === 'normal' && t.id === running[0]?.id));
  const isOpen = (t: BgTask) => toggled.has(t.id) !== openByDefault(t);
  const toggle = (id: string) => () =>
    setToggled((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const item = (t: BgTask) => (
    <TaskItem key={t.id} task={t} now={now} open={isOpen(t)} visible={visible} detail={detail} onToggle={toggle(t.id)} />
  );

  return (
    <div className={styles.root}>
      {tasks.length === 0 && (
        <p className={styles.empty}>
          No tasks yet. Subagents, background shells and monitors the agent starts, and other agent CLIs it runs, show up here.
        </p>
      )}
      {running.map(item)}
      {finished.length > 0 && (
        <div className={styles.group}>
          <div className={styles.groupHead}>
            <button
              type="button"
              className={styles.groupToggle}
              onClick={() => setFinishedOpen((o) => !o)}
              aria-expanded={finishedOpen}
            >
              {finishedOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              Finished {finished.length}
            </button>
            <button
              type="button"
              className={styles.clear}
              onClick={() => clearFinished(sourceId)}
              aria-label="Clear finished tasks"
              data-tooltip="Clear finished"
            >
              <Trash2 size={12} />
            </button>
          </div>
          {finishedOpen && finished.map(item)}
        </div>
      )}
    </div>
  );
};

export default TasksTile;
