import type { DockLayout, DockMode, DockSide, PanelId } from '~/domain/interfaces/dock.interface';

import { getPref, setPref } from '~/usecase/util/prefs';
import { movePanel, hidePanel, showPanel, setActivePanel, resetLayout, normalizeLayout } from '~/usecase/util/dockLayout';

type Listener = () => void;

const listeners = new Map<DockMode, Set<Listener>>();
const snapshots = new Map<DockMode, DockLayout>();

const prefKey = (mode: DockMode): string => `panorama:dock:${mode}`;

const load = (mode: DockMode): DockLayout => {
  try {
    const raw = getPref(prefKey(mode));
    if (!raw) return normalizeLayout(mode, null);
    return normalizeLayout(mode, JSON.parse(raw) as unknown);
  } catch {
    return normalizeLayout(mode, null);
  }
};

const save = (mode: DockMode, layout: DockLayout): void => {
  setPref(prefKey(mode), JSON.stringify(layout));
};

const notify = (mode: DockMode): void => {
  listeners.get(mode)?.forEach((fn) => fn());
};

const getLayout = (mode: DockMode): DockLayout => {
  if (!snapshots.has(mode)) snapshots.set(mode, load(mode));
  return snapshots.get(mode)!;
};

const setLayout = (mode: DockMode, layout: DockLayout): void => {
  snapshots.set(mode, layout);
  save(mode, layout);
  notify(mode);
};

export const getDockSnapshot = (mode: DockMode): DockLayout => getLayout(mode);

export const subscribeDock = (mode: DockMode, fn: Listener): (() => void) => {
  if (!listeners.has(mode)) listeners.set(mode, new Set());
  listeners.get(mode)!.add(fn);
  return () => listeners.get(mode)!.delete(fn);
};

export const dockLayoutActions = {
  move: (mode: DockMode, id: PanelId, side: DockSide): void => setLayout(mode, movePanel(getLayout(mode), id, side)),
  hide: (mode: DockMode, id: PanelId): void => setLayout(mode, hidePanel(getLayout(mode), id)),
  show: (mode: DockMode, id: PanelId): void => setLayout(mode, showPanel(getLayout(mode), id, mode)),
  setActive: (mode: DockMode, side: DockSide, id: PanelId): void => setLayout(mode, setActivePanel(getLayout(mode), side, id)),
  reset: (mode: DockMode): void => setLayout(mode, resetLayout(mode))
};
