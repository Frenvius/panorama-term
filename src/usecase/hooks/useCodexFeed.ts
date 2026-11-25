import type { BgTask } from '~/usecase/util/bgTasks';

import React from 'react';

import { readDir, homeDir } from '~/adapter/fs/fs.client';
import { useFileTail } from '~/usecase/hooks/useFileTail';
import { parseRollout, pickRollout, rolloutDirs } from '~/usecase/util/codexFeed';

const FIND_MS = 1500;
// Log rows carry whole command outputs; a wide tail keeps more than the last few of them.
const TAIL_BYTES = 512 * 1024;

// Which log each task follows, so two runs never take the same one.
const claimed = new Map<string, string>();

const findRollout = async (task: BgTask): Promise<string | undefined> => {
  const home = await homeDir();
  if (!home) return undefined;
  const taken = new Set(claimed.values());
  for (const dir of rolloutDirs(home, task.startedAt, task.endedAt ?? Date.now())) {
    const entries = await readDir(dir);
    const name = pickRollout(entries.filter((e) => !e.dir).map((e) => e.name), task.startedAt, taken);
    if (name) return `${dir}/${name}`;
  }
  return undefined;
};

// What a Codex run is doing, while `enabled`: finds its session log, then follows it.
export const useCodexFeed = (task: BgTask, enabled: boolean) => {
  const [path, setPath] = React.useState(() => claimed.get(task.id));
  const running = !task.endedAt;

  React.useEffect(() => {
    if (!enabled || path) return;
    let alive = true;
    const look = () =>
      void findRollout(task)
        .then((found) => {
          if (!alive || !found || claimed.has(task.id)) return;
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
