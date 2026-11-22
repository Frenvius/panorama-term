import type { BgTask } from '~/usecase/util/bgTasks';

import React from 'react';
import { Trash2, ChevronDown, ChevronRight } from 'lucide-react';

import { formatElapsed } from '~/usecase/util/elapsed';
import { useFileTail } from '~/usecase/hooks/useFileTail';
import { useBgTasks, clearFinished } from '~/usecase/util/bgTasks';

import styles from './styles.module.scss';

const STATUS_LABEL: Record<string, string> = { completed: 'done', failed: 'failed', killed: 'stopped' };

interface TaskItemProps {
  task: BgTask;
  now: number;
  open: boolean;
  onToggle: () => void;
}

const TaskItem = ({ task, now, open, onToggle }: TaskItemProps) => {
  const { text, error } = useFileTail(open ? task.outputFile : undefined);
  const outRef = React.useRef<HTMLPreElement>(null);

  React.useLayoutEffect(() => {
    if (outRef.current) outRef.current.scrollTop = outRef.current.scrollHeight;
  }, [text]);

  return (
    <div className={styles.task} data-ended={Boolean(task.endedAt)}>
      <button type="button" className={styles.taskHead} onClick={onToggle} aria-expanded={open} title={task.command}>
        {open ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        {!task.endedAt && <span className={styles.dot} />}
        <span className={styles.title}>{task.description || task.command}</span>
        {task.status && (
          <span className={styles.status} data-status={task.status}>
            {STATUS_LABEL[task.status] ?? task.status}
          </span>
        )}
        <span className={styles.elapsed}>{formatElapsed((task.endedAt ?? now) - task.startedAt)}</span>
      </button>
      {open && (
        <div className={styles.taskBody}>
          <pre className={styles.command}>$ {task.command}</pre>
          <pre ref={outRef} className={styles.output}>
            {task.outputFile ? error ?? text : 'Output file unknown for this task.'}
          </pre>
        </div>
      )}
    </div>
  );
};

const TasksTile = ({ sourceId }: { sourceId: string }) => {
  const tasks = useBgTasks(sourceId);
  const [toggled, setToggled] = React.useState<Set<string>>(() => new Set());
  const [finishedOpen, setFinishedOpen] = React.useState(true);
  const [now, setNow] = React.useState(Date.now);
  const running = tasks.filter((t) => !t.endedAt);
  const finished = tasks.filter((t) => t.endedAt);
  const anyRunning = running.length > 0;

  React.useEffect(() => {
    if (!anyRunning) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [anyRunning]);

  // The newest running task starts expanded; clicking flips whatever the default was.
  const isOpen = (t: BgTask) => toggled.has(t.id) !== (t.id === running[0]?.id);
  const toggle = (id: string) => () =>
    setToggled((prev) => {
      const next = new Set(prev);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  const item = (t: BgTask) => <TaskItem key={t.id} task={t} now={now} open={isOpen(t)} onToggle={toggle(t.id)} />;

  return (
    <div className={styles.root}>
      {tasks.length === 0 && <p className={styles.empty}>No background tasks yet. Shells the agent starts in the background show up here.</p>}
      {running.map(item)}
      {finished.length > 0 && (
        <div className={styles.group}>
          <div className={styles.groupHead}>
            <button type="button" className={styles.groupToggle} onClick={() => setFinishedOpen((o) => !o)} aria-expanded={finishedOpen}>
              {finishedOpen ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
              Finished {finished.length}
            </button>
            <button type="button" className={styles.clear} onClick={() => clearFinished(sourceId)} aria-label="Clear finished tasks" data-tooltip="Clear finished">
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
