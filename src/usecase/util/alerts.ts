import { invoke } from '@tauri-apps/api/core';

import type { NotifyKind } from '~/domain/interfaces/notify.interface';

export interface TileAlert {
  kind: NotifyKind;
  tabId: string;
}

const ALERTS_KEY = 'panorama:alerts';

const listeners = new Set<() => void>();

const load = (): Map<string, TileAlert> => {
  try {
    const raw = localStorage.getItem(ALERTS_KEY);
    if (!raw) return new Map();
    const parsed = JSON.parse(raw) as Record<string, NotifyKind | TileAlert>;
    const entries = Object.entries(parsed).map<[string, TileAlert]>(([id, value]) =>
      typeof value === 'string' ? [id, { kind: value, tabId: '' }] : [id, value]
    );
    return new Map(entries);
  } catch {
    return new Map();
  }
};

let alerts = load();

void invoke('set_pending_count', { count: alerts.size }).catch(() => {});

const commit = (next: Map<string, TileAlert>): void => {
  alerts = next;
  localStorage.setItem(ALERTS_KEY, JSON.stringify(Object.fromEntries(next)));
  void invoke('set_pending_count', { count: next.size }).catch(() => {});
  listeners.forEach((fn) => fn());
};

export const getAlerts = (): Map<string, TileAlert> => alerts;

export const subscribeAlerts = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const setAlert = (tileId: string, kind: NotifyKind, tabId: string): void => {
  const current = alerts.get(tileId);
  if (current?.kind === kind && current.tabId === tabId) return;
  commit(new Map(alerts).set(tileId, { kind, tabId }));
};

export const clearAlert = (tileId: string): void => {
  if (!alerts.has(tileId)) return;
  const next = new Map(alerts);
  next.delete(tileId);
  commit(next);
};

export const clearTabAlerts = (tabId: string): void => {
  const next = new Map([...alerts].filter(([, alert]) => alert.tabId !== tabId));
  if (next.size === alerts.size) return;
  commit(next);
};

export const hasTabAlert = (tabId: string): boolean => {
  for (const alert of alerts.values()) if (alert.tabId === tabId) return true;
  return false;
};
