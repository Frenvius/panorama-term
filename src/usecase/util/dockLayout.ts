import type { DockLayout, DockMode, DockSide, PanelId } from '~/domain/interfaces/dock.interface';

export const MODE_PANELS: Record<DockMode, PanelId[]> = {
  canvas: ['files', 'projects', 'git', 'docker', 'orch'],
  project: ['projects', 'files', 'git', 'notes', 'docker', 'orch']
};

export const DEFAULT_LAYOUT: Record<DockMode, DockLayout> = {
  canvas: {
    left: ['files', 'projects', 'git', 'docker', 'orch'],
    right: [],
    hidden: [],
    active: { left: 'files', right: null }
  },
  project: {
    left: ['projects'],
    right: ['files', 'git', 'notes', 'docker', 'orch'],
    hidden: [],
    active: { left: 'projects', right: 'files' }
  }
};

const clone = (layout: DockLayout): DockLayout => JSON.parse(JSON.stringify(layout)) as DockLayout;

export const normalizeLayout = (mode: DockMode, raw: unknown): DockLayout => {
  const allowed = new Set(MODE_PANELS[mode]);
  const defaults = DEFAULT_LAYOUT[mode];

  if (!raw || typeof raw !== 'object' || Array.isArray(raw)) return clone(defaults);

  const migrateId = (v: unknown): unknown => (v === 'tiles' ? 'projects' : v);
  const migrateArr = (arr: unknown): unknown[] => (Array.isArray(arr) ? arr.map(migrateId) : []);

  const rawObj = raw as Record<string, unknown>;
  const rawActive = rawObj.active && typeof rawObj.active === 'object' && !Array.isArray(rawObj.active)
    ? rawObj.active as Record<string, unknown>
    : {};
  const r: Record<string, unknown> = {
    ...rawObj,
    left: migrateArr(rawObj.left),
    right: migrateArr(rawObj.right),
    hidden: migrateArr(rawObj.hidden),
    active: { left: migrateId(rawActive.left), right: migrateId(rawActive.right) }
  };

  const toIds = (arr: unknown): PanelId[] => {
    if (!Array.isArray(arr)) return [];
    const seen = new Set<PanelId>();
    const result: PanelId[] = [];
    for (const item of arr) {
      if (typeof item === 'string' && allowed.has(item as PanelId) && !seen.has(item as PanelId)) {
        seen.add(item as PanelId);
        result.push(item as PanelId);
      }
    }
    return result;
  };

  const left = toIds(r.left);
  const right = toIds(r.right).filter((id) => !left.includes(id));
  const hidden = toIds(r.hidden).filter((id) => !left.includes(id) && !right.includes(id));
  const placed = new Set([...left, ...right, ...hidden]);

  for (const id of MODE_PANELS[mode]) {
    if (!placed.has(id)) {
      if (defaults.left.includes(id)) left.push(id);
      else if (defaults.right.includes(id)) right.push(id);
      else hidden.push(id);
    }
  }

  const migratedActive =
    r.active && typeof r.active === 'object' && !Array.isArray(r.active) ? (r.active as Record<string, unknown>) : {};

  const fixActive = (side: PanelId[], rawId: unknown): PanelId | null => {
    if (side.length === 0) return null;
    if (typeof rawId === 'string' && side.includes(rawId as PanelId)) return rawId as PanelId;
    return side[0] ?? null;
  };

  return {
    left,
    right,
    hidden,
    active: {
      left: fixActive(left, migratedActive.left),
      right: fixActive(right, migratedActive.right)
    }
  };
};

const removePanelFromLayout = (layout: DockLayout, id: PanelId): DockLayout => {
  const left = layout.left.filter((p) => p !== id);
  const right = layout.right.filter((p) => p !== id);
  const hidden = layout.hidden.filter((p) => p !== id);
  const activeLeft = layout.active.left === id ? (left[0] ?? null) : layout.active.left;
  const activeRight = layout.active.right === id ? (right[0] ?? null) : layout.active.right;
  return { left, right, hidden, active: { left: activeLeft, right: activeRight } };
};

export const movePanel = (layout: DockLayout, id: PanelId, side: DockSide): DockLayout => {
  const without = removePanelFromLayout(layout, id);
  const left = side === 'left' ? [...without.left, id] : without.left;
  const right = side === 'right' ? [...without.right, id] : without.right;
  return {
    left,
    right,
    hidden: without.hidden,
    active: {
      left: side === 'left' ? id : without.active.left,
      right: side === 'right' ? id : without.active.right
    }
  };
};

export const hidePanel = (layout: DockLayout, id: PanelId): DockLayout => {
  const without = removePanelFromLayout(layout, id);
  return { ...without, hidden: [...without.hidden, id] };
};

export const showPanel = (layout: DockLayout, id: PanelId, mode: DockMode): DockLayout => {
  const without = removePanelFromLayout(layout, id);
  const defaults = DEFAULT_LAYOUT[mode];
  const defaultSide: DockSide = defaults.left.includes(id) ? 'left' : 'right';
  const left = defaultSide === 'left' ? [...without.left, id] : without.left;
  const right = defaultSide === 'right' ? [...without.right, id] : without.right;
  return {
    left,
    right,
    hidden: without.hidden,
    active: {
      left: defaultSide === 'left' ? id : without.active.left,
      right: defaultSide === 'right' ? id : without.active.right
    }
  };
};

export const setActivePanel = (layout: DockLayout, side: DockSide, id: PanelId): DockLayout => ({
  ...layout,
  active: { ...layout.active, [side]: id }
});

export const resetLayout = (mode: DockMode): DockLayout => clone(DEFAULT_LAYOUT[mode]);

export const visiblePanels = (layout: DockLayout, side: DockSide, available: (id: PanelId) => boolean): PanelId[] =>
  layout[side].filter(available);
