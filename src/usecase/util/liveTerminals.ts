const live = new Set<string>();
const listeners = new Set<() => void>();

let version = 0;

export const isTerminalLive = (tileId: string): boolean => live.has(tileId);

export const getLiveVersion = (): number => version;

export const subscribeLiveTerminals = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const markTerminalLive = (tileId: string, value: boolean): void => {
  if (value === live.has(tileId)) return;
  if (value) live.add(tileId);
  else live.delete(tileId);
  version += 1;
  listeners.forEach((fn) => fn());
};
