import React from 'react';

import type { RunStatus } from '~/domain/interfaces/run.interface';
import type { RunKind } from '~/adapter/run/run.client';
import { storeRead, storeWrite } from '~/adapter/store/store.client';
import { watchRun, startRun, stopRun, refreshRun, detectRunCommands, watchRunManifests } from '~/adapter/run/run.client';

const DEFAULTS_STORE = 'run-defaults';

type Defaults = Record<string, string>;

let defaultsCache: Defaults | null = null;

const loadDefaults = async (): Promise<Defaults> => {
  if (!defaultsCache) defaultsCache = (await storeRead<Defaults>(DEFAULTS_STORE)) ?? {};
  return defaultsCache;
};

const saveDefault = async (key: string, cmd: string): Promise<void> => {
  const all = await loadDefaults();
  all[key] = cmd;
  await storeWrite(DEFAULTS_STORE, all);
};

const BUILD_RE = /\b(build|compile|bundle|dist|tsc)\b/i;

const isBuild = (cmd: string): boolean => cmd === 'cargo build' || BUILD_RE.test(cmd);

const filterKind = (commands: string[], kind: RunKind): string[] =>
  commands.filter((c) => (kind === 'build' ? isBuild(c) : !isBuild(c)));

const pickDefault = (commands: string[], kind: RunKind, stored?: string): string | null => {
  if (stored && commands.includes(stored)) return stored;
  if (kind === 'build') {
    return commands.find((c) => c === 'bun run build') ?? commands.find((c) => c === 'cargo build') ?? commands[0] ?? null;
  }
  return (
    commands.find((c) => c === 'bun run dev') ??
    commands.find((c) => c === 'bun run start') ??
    commands[0] ??
    null
  );
};

export const useRun = (cwd: string | undefined, tile: string | undefined, kind: RunKind = 'run') => {
  const [status, setStatus] = React.useState<RunStatus>({ state: 'none' });
  const [allCommands, setAllCommands] = React.useState<string[]>([]);
  const [stored, setStored] = React.useState<string | undefined>(undefined);
  const [crashed, setCrashed] = React.useState<RunStatus | null>(null);
  const prevRef = React.useRef<RunStatus>({ state: 'none' });
  const storeKey = cwd ? `${kind}:${cwd}` : '';

  React.useEffect(() => {
    if (!cwd) return;
    let alive = true;
    let debounce: ReturnType<typeof setTimeout> | undefined;
    const detect = () => {
      void detectRunCommands(cwd).then((cmds) => {
        if (alive) setAllCommands(cmds);
      });
    };
    detect();
    void loadDefaults().then((all) => {
      if (alive) setStored(all[`${kind}:${cwd}`]);
    });
    const unwatch = watchRunManifests(cwd, () => {
      clearTimeout(debounce);
      debounce = setTimeout(detect, 300);
    });
    return () => {
      alive = false;
      clearTimeout(debounce);
      unwatch();
    };
  }, [cwd, kind]);

  React.useEffect(() => {
    if (!tile) return;
    prevRef.current = { state: 'none' };
    return watchRun(
      tile,
      (s) => {
        const prev = prevRef.current;
        prevRef.current = s;
        setStatus(s);
        if (prev.state === 'running' && s.state === 'exited' && (s.exitCode ?? 0) !== 0) setCrashed(s);
      },
      kind
    );
  }, [tile, kind]);

  const commands = React.useMemo(() => filterKind(allCommands, kind), [allCommands, kind]);
  const defaultCmd = pickDefault(commands, kind, stored);

  const start = React.useCallback(
    (cmd?: string) => {
      if (!cwd || !tile) return;
      const chosen = cmd ?? pickDefault(filterKind(allCommands, kind), kind, stored);
      if (!chosen) return;
      if (cmd) {
        setStored(cmd);
        void saveDefault(storeKey, cmd);
      }
      void startRun(cwd, chosen, tile, kind).then(() => refreshRun(tile, kind));
    },
    [cwd, tile, kind, allCommands, stored, storeKey]
  );

  const stop = React.useCallback(
    (hard = false) => {
      if (!tile) return;
      void stopRun(tile, hard, kind).then(() => refreshRun(tile, kind));
    },
    [tile, kind]
  );

  const restart = React.useCallback(() => {
    if (!cwd || !tile) return;
    const cmd = status.cmd ?? pickDefault(filterKind(allCommands, kind), kind, stored);
    if (!cmd) return;
    void stopRun(tile, true, kind).then(() => {
      const tryStart = (left: number) => {
        void startRun(cwd, cmd, tile, kind).then((r) => {
          if (r.error && left > 0) setTimeout(() => tryStart(left - 1), 300);
          else refreshRun(tile, kind);
        });
      };
      tryStart(10);
    });
  }, [cwd, tile, kind, status.cmd, allCommands, stored]);

  const clearCrashed = React.useCallback(() => setCrashed(null), []);

  return { status, commands, defaultCmd, crashed, clearCrashed, start, stop, restart };
};
