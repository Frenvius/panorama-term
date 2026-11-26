import type { BgTask } from '~/usecase/util/bgTasks';

import React from 'react';

import { readDir, readFileHead, homeDir } from '~/adapter/fs/fs.client';
import { useFileTail } from '~/usecase/hooks/useFileTail';
import { findRollout, headStart, parseRollout } from '~/usecase/util/codexFeed';

const FIND_MS = 1500;
// Log rows carry whole command outputs; a wide tail keeps more than the last few of them.
const TAIL_BYTES = 512 * 1024;
const HEAD_BYTES = 256;

// Which log each task follows, so two runs never take the same one.
const claimed = new Map<string, string>();
// When each log began; a log's first line never changes.
const starts = new Map<string, number>();

const io = {
  list: async (dir: string) => (await readDir(dir)).filter((e) => !e.dir).map((e) => e.name),
  start: async (path: string) => {
    const known = starts.get(path);
    if (known !== undefined) return known;
    const at = headStart(await readFileHead(path, HEAD_BYTES));
    if (!Number.isNaN(at)) starts.set(path, at);
    return at;
  }
};

const lookUp = async (task: BgTask): Promise<string | undefined> => {
  const home = await homeDir();
  return home ? findRollout(home, task.startedAt, task.endedAt ?? Date.now(), new Set(claimed.values()), io) : undefined;
};

// What a Codex run is doing, while `enabled`: finds its session log, then follows it.
export const useCodexFeed = (task: BgTask, enabled: boolean) => {
  const [path, setPath] = React.useState(() => claimed.get(task.id));
  const running = !task.endedAt;

  React.useEffect(() => {
    if (!enabled || path) return;
    let alive = true;
    const look = () =>
      void lookUp(task)
        .then((found) => {
          // Another task's lookup may have taken the same log meanwhile.
          if (!alive || !found || claimed.has(task.id) || [...claimed.values()].includes(found)) return;
          claimed.set(task.id, found);
          setPath(found);
        })
        .catch(() => {});
    look();
    const timer = running ? window.setInterval(look, FIND_MS) : undefined;
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [enabled, path, running, task.id]);

  const { text } = useFileTail(enabled ? path : undefined, { maxBytes: TAIL_BYTES, live: running });
  const lines = React.useMemo(() => parseRollout(text), [text]);
  return { found: Boolean(path), lines };
};
