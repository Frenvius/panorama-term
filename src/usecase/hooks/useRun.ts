import React from 'react';

import type { RunPhase, RunStatus } from '~/domain/interfaces/run.interface';
import type { RunKind } from '~/adapter/run/run.client';
import { storeRead, storeWrite } from '~/adapter/store/store.client';
import { watchRun, startRun, stopRun, refreshRun, restartRun, fetchRunStatus, detectRunCommands, watchRunManifests } from '~/adapter/run/run.client';

const DEFAULTS_STORE = 'run-defaults';
const PHASE_POLL_MS = 500;

const serverPhase = (status: RunStatus, kind: RunKind): RunPhase | null => {
  if (status.state !== 'running') return null;
  if (status.phase) return status.phase;
  return kind === 'run' && !status.ready ? 'starting' : null;
};

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
  const [pending, setPending] = React.useState<RunPhase | null>(null);
  const [restartedPid, setRestartedPid] = React.useState<number | null>(null);
  const prevRef = React.useRef<RunStatus>({ state: 'none' });
  const storeKey = cwd ? `${kind}:${cwd}` : '';
  const fromServer = serverPhase(status, kind);
  const restarting = fromServer === 'starting' && restartedPid !== null && status.pid === restartedPid;
  const phase = pending ?? (restarting ? 'restarting' : fromServer);

  const apply = React.useCallback((s: RunStatus) => {
    const prev = prevRef.current;
    prevRef.current = s;
    setStatus(s);
    if (prev.state === 'running' && s.state === 'exited' && (s.exitCode ?? 0) !== 0 && !s.stoppedByUser) setCrashed(s);
  }, []);

  const settle = React.useCallback(() => {
    if (!tile) return;
    void fetchRunStatus(tile, kind).then((s) => {
      apply(s);
      setPending(null);
    });
  }, [tile, kind, apply]);

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
    return watchRun(tile, apply, kind);
  }, [tile, kind, apply]);

  React.useEffect(() => {
    if (!phase || !tile) return;
    const t = setInterval(() => refreshRun(tile, kind), PHASE_POLL_MS);
    return () => clearInterval(t);
  }, [phase, tile, kind]);

  React.useEffect(() => {
    if (restartedPid === null) return;
    if (status.state === 'exited' || (status.pid === restartedPid && fromServer !== 'starting')) setRestartedPid(null);
  }, [restartedPid, status.state, status.pid, fromServer]);

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
      setPending('starting');
      void startRun(cwd, chosen, tile, kind).then(settle);
    },
    [cwd, tile, kind, allCommands, stored, storeKey, settle]
  );

  const stop = React.useCallback(
    (hard = false) => {
      if (!tile) return;
      setPending(hard ? 'killing' : 'stopping');
      void stopRun(tile, hard, kind).then(settle);
    },
    [tile, kind, settle]
  );

  const restart = React.useCallback(() => {
    if (!tile) return;
    if (status.state === 'none') {
      start();
      return;
    }
    setPending('restarting');
    void restartRun(tile, kind).then((r) => {
      if (!r.error && r.pid) setRestartedPid(r.pid);
      settle();
    });
  }, [tile, kind, status.state, start, settle]);

  const clearCrashed = React.useCallback(() => setCrashed(null), []);

  return { status, phase, lastLine: status.lastLine ?? '', commands, defaultCmd, crashed, clearCrashed, start, stop, restart };
};
