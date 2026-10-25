import React from 'react';

import type { RunPhase, RunStatus } from '~/domain/interfaces/run.interface';
import type { RunKind } from '~/adapter/run/run.client';
import { storeRead, storeWrite } from '~/adapter/store/store.client';
import { watchRun, startRun, stopRun, refreshRun, fetchRunTail, detectRunCommands, watchRunManifests } from '~/adapter/run/run.client';

const DEFAULTS_STORE = 'run-defaults';
const PHASE_POLL_MS = 400;
const READY_POLL_MS = 500;
const READY_TIMEOUT_MS = 30000;
const GRACEFUL_STOP_MS = 3000;
const LAST_LINE_MAX = 80;
const READY_RE = /\b(ready|listening|compiled successfully|server running|running at)\b|local:\s+https?:\/\/|https?:\/\/(localhost|127\.0\.0\.1|0\.0\.0\.0)[:/]/i;

const clipLine = (line: string): string => {
  const flat = line.replace(/\s+/g, ' ').trim();
  return flat.length > LAST_LINE_MAX ? `${flat.slice(0, LAST_LINE_MAX - 3)}...` : flat;
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
  const [phase, setPhase] = React.useState<RunPhase | null>(null);
  const [lastLine, setLastLine] = React.useState('');
  const [spawnedAt, setSpawnedAt] = React.useState<number | null>(null);
  const prevRef = React.useRef<RunStatus>({ state: 'none' });
  const phaseRef = React.useRef<RunPhase | null>(null);
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
        if (prev.state === 'running' && s.state === 'exited' && (s.exitCode ?? 0) !== 0 && !phaseRef.current) setCrashed(s);
      },
      kind
    );
  }, [tile, kind]);

  React.useEffect(() => {
    phaseRef.current = phase;
  }, [phase]);

  React.useEffect(() => {
    if ((phase === 'stopping' || phase === 'killing') && status.state !== 'running') setPhase(null);
    if (spawnedAt !== null && status.state === 'exited') {
      setPhase(null);
      setSpawnedAt(null);
    }
  }, [phase, status.state, spawnedAt]);

  React.useEffect(() => {
    if (!phase || !tile) return;
    const t = setInterval(() => refreshRun(tile, kind), PHASE_POLL_MS);
    return () => clearInterval(t);
  }, [phase, tile, kind]);

  React.useEffect(() => {
    if (phase !== 'stopping') return;
    const t = setTimeout(() => setPhase('killing'), GRACEFUL_STOP_MS);
    return () => clearTimeout(t);
  }, [phase]);

  React.useEffect(() => {
    if (spawnedAt === null || !tile) return;
    let alive = true;
    const check = () => {
      void fetchRunTail(tile, kind, 50).then((lines) => {
        if (!alive) return;
        const last = [...lines].reverse().find((line) => line.trim());
        if (last) setLastLine(clipLine(last));
        if (!lines.some((line) => READY_RE.test(line)) && Date.now() - spawnedAt < READY_TIMEOUT_MS) return;
        setPhase(null);
        setSpawnedAt(null);
      });
    };
    check();
    const t = setInterval(check, READY_POLL_MS);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [spawnedAt, tile, kind]);

  const commands = React.useMemo(() => filterKind(allCommands, kind), [allCommands, kind]);
  const defaultCmd = pickDefault(commands, kind, stored);

  const onSpawned = React.useCallback(
    (r: RunStatus) => {
      if (!tile) return;
      refreshRun(tile, kind);
      if (r.error) setPhase(null);
      else if (kind === 'build') setPhase(null);
      else setSpawnedAt(Date.now());
    },
    [tile, kind]
  );

  const start = React.useCallback(
    (cmd?: string) => {
      if (!cwd || !tile) return;
      const chosen = cmd ?? pickDefault(filterKind(allCommands, kind), kind, stored);
      if (!chosen) return;
      if (cmd) {
        setStored(cmd);
        void saveDefault(storeKey, cmd);
      }
      setPhase('starting');
      setLastLine('');
      void startRun(cwd, chosen, tile, kind).then(onSpawned);
    },
    [cwd, tile, kind, allCommands, stored, storeKey, onSpawned]
  );

  const stop = React.useCallback(
    (hard = false) => {
      if (!tile) return;
      setPhase(hard ? 'killing' : 'stopping');
      setSpawnedAt(null);
      void stopRun(tile, hard, kind).then(() => refreshRun(tile, kind));
    },
    [tile, kind]
  );

  const restart = React.useCallback(() => {
    if (!cwd || !tile) return;
    const cmd = status.cmd ?? pickDefault(filterKind(allCommands, kind), kind, stored);
    if (!cmd) return;
    setPhase('restarting');
    setLastLine('');
    setSpawnedAt(null);
    void stopRun(tile, true, kind).then(() => {
      const tryStart = (left: number) => {
        void startRun(cwd, cmd, tile, kind).then((r) => {
          if (r.error && left > 0) setTimeout(() => tryStart(left - 1), 300);
          else onSpawned(r);
        });
      };
      tryStart(10);
    });
  }, [cwd, tile, kind, status.cmd, allCommands, stored, onSpawned]);

  const clearCrashed = React.useCallback(() => setCrashed(null), []);

  return { status, phase, lastLine, commands, defaultCmd, crashed, clearCrashed, start, stop, restart };
};
