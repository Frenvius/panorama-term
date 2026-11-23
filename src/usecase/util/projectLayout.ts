import type { Tile } from '~/domain/interfaces/workspace.interface';
import type {
  DropZone,
  PanelGroup,
  SplitDirection,
  PanelLayoutNode,
  ProjectViewState,
  ProjectPanelState
} from '~/domain/interfaces/projectView.interface';

const MIN_SPLIT_RATIO = 0.1;
const MAX_SPLIT_RATIO = 0.9;

const newGroupId = (): string => `grp-${Math.random().toString(36).slice(2, 10)}`;

export const DEFAULT_PROJECT = '~';
export const GLOBAL_PANEL = '*';

export const normalizeProjectPath = (raw: string): string => {
  const trimmed = raw.trim().replace(/\\/g, '/');
  if (!trimmed) return '';
  if (trimmed === DEFAULT_PROJECT) return DEFAULT_PROJECT;
  if (trimmed === '/') return '/';
  return trimmed.replace(/\/+$/, '');
};

const WORKTREE_SEGMENT = '/.claude/worktrees/';

export const projectRootPath = (raw: string): string => {
  const norm = normalizeProjectPath(raw);
  const idx = norm.toLowerCase().indexOf(WORKTREE_SEGMENT);
  return idx > 0 ? norm.slice(0, idx) : norm;
};

export const projectName = (projectPath: string): string => {
  const norm = normalizeProjectPath(projectPath);
  if (norm === DEFAULT_PROJECT) return 'Workspace';
  if (!norm || norm === '/') return norm;
  const parts = norm.split('/');
  return parts[parts.length - 1] || norm;
};

export const pathStartsWith = (targetPath: string, prefixPath: string): boolean => {
  const target = normalizeProjectPath(targetPath).toLowerCase();
  const prefix = normalizeProjectPath(prefixPath).toLowerCase();
  if (!target || !prefix) return false;
  if (prefix === '/') return target.startsWith('/');
  return target === prefix || target.startsWith(`${prefix}/`);
};

export const tileCandidatePath = (tile: Tile): string => tile.workspacePath ?? tile.folderPath ?? tile.cwd ?? tile.filePath ?? '';

export const tileProject = (tile: Tile, projects: string[]): string | null => {
  if (projects.length === 0) return null;
  const normalizedProjects = projects.map(normalizeProjectPath).filter(Boolean);
  if (tile.workspacePath) {
    const wsNorm = projectRootPath(tile.workspacePath).toLowerCase();
    const exact = normalizedProjects.find((p) => p.toLowerCase() === wsNorm);
    if (exact) return exact;
  }
  const candidate = tileCandidatePath(tile);
  if (!candidate) {
    if (tile.type === 'note') return normalizedProjects[0] ?? null;
    return normalizedProjects.find((p) => p === DEFAULT_PROJECT) ?? null;
  }
  let bestMatch: string | null = null;
  for (const proj of normalizedProjects) {
    if (pathStartsWith(candidate, proj)) {
      if (!bestMatch || proj.length > bestMatch.length) bestMatch = proj;
    }
  }
  return bestMatch;
};

export const tileMatchesProject = (tile: Tile, projectPath: string, allProjects?: string[]): boolean => {
  const normTarget = normalizeProjectPath(projectPath);
  if (!normTarget) return false;
  const list = allProjects && allProjects.length > 0 ? allProjects : [normTarget];
  const matched = tileProject(tile, list);
  return matched !== null && matched.toLowerCase() === normTarget.toLowerCase();
};

export const collectGroupIds = (node: PanelLayoutNode): string[] => {
  if (node.type === 'leaf') return [node.groupId];
  return [...collectGroupIds(node.first), ...collectGroupIds(node.second)];
};

export const emptyProjectPanelState = (tileIds: string[] = [], groupId: string = newGroupId()): ProjectPanelState => {
  const uniqueIds = Array.from(new Set(tileIds));
  return {
    layout: { type: 'leaf', groupId },
    activeGroupId: groupId,
    maximizedGroupId: null,
    groups: {
      [groupId]: {
        id: groupId,
        tileIds: uniqueIds,
        activeTileId: uniqueIds[0] ?? null
      }
    }
  };
};

const pruneNode = (node: PanelLayoutNode, validGroups: Set<string>): PanelLayoutNode | null => {
  if (node.type === 'leaf') return validGroups.has(node.groupId) ? node : null;
  const first = pruneNode(node.first, validGroups);
  const second = pruneNode(node.second, validGroups);
  if (first && second) {
    if (first === node.first && second === node.second) return node;
    return { ...node, first, second };
  }
  return first ?? second;
};

const replaceLeafWithSplit = (node: PanelLayoutNode, targetGroupId: string, replacement: PanelLayoutNode): PanelLayoutNode => {
  if (node.type === 'leaf') return node.groupId === targetGroupId ? replacement : node;
  const first = replaceLeafWithSplit(node.first, targetGroupId, replacement);
  const second = replaceLeafWithSplit(node.second, targetGroupId, replacement);
  if (first === node.first && second === node.second) return node;
  return { ...node, first, second };
};

export interface ActivateTileOpts {
  tileId: string;
  groupId?: string;
}

export const activateTileInProject = (state: ProjectPanelState, opts: ActivateTileOpts): ProjectPanelState => {
  const { tileId, groupId } = opts;
  const existingGroup = Object.values(state.groups).find((g) => g.tileIds.includes(tileId));

  if (existingGroup && (!groupId || groupId === existingGroup.id)) {
    return {
      ...state,
      activeGroupId: existingGroup.id,
      groups: {
        ...state.groups,
        [existingGroup.id]: {
          ...existingGroup,
          activeTileId: tileId
        }
      }
    };
  }

  const leafIds = collectGroupIds(state.layout);
  const targetId =
    (groupId && state.groups[groupId] ? groupId : undefined) ??
    (state.groups[state.activeGroupId] ? state.activeGroupId : leafIds[0]);

  if (!targetId || !state.groups[targetId]) {
    return emptyProjectPanelState([tileId], groupId);
  }

  const nextGroups: Record<string, PanelGroup> = { ...state.groups };
  if (existingGroup && existingGroup.id !== targetId) {
    const filtered = existingGroup.tileIds.filter((id) => id !== tileId);
    nextGroups[existingGroup.id] = {
      ...existingGroup,
      tileIds: filtered,
      activeTileId: existingGroup.activeTileId === tileId ? (filtered[0] ?? null) : existingGroup.activeTileId
    };
  }

  const target = nextGroups[targetId]!;
  const nextTileIds = target.tileIds.includes(tileId) ? target.tileIds : [...target.tileIds, tileId];
  nextGroups[targetId] = {
    ...target,
    tileIds: nextTileIds,
    activeTileId: tileId
  };

  return {
    ...state,
    activeGroupId: targetId,
    groups: nextGroups
  };
};

export const closePanelGroup = (state: ProjectPanelState, groupId: string): ProjectPanelState => {
  if (!state.groups[groupId]) return state;
  const nextGroups: Record<string, PanelGroup> = {};
  for (const [id, group] of Object.entries(state.groups)) {
    if (id !== groupId) nextGroups[id] = group;
  }
  const validIds = new Set(Object.keys(nextGroups));
  const pruned = pruneNode(state.layout, validIds);
  if (!pruned || validIds.size === 0) {
    return emptyProjectPanelState([], groupId);
  }
  const remainingOrder = collectGroupIds(pruned);
  const nextActiveGroupId = validIds.has(state.activeGroupId)
    ? state.activeGroupId
    : (remainingOrder[0] ?? Object.keys(nextGroups)[0]!);
  const nextMaximized =
    remainingOrder.length > 1 && state.maximizedGroupId && validIds.has(state.maximizedGroupId) ? state.maximizedGroupId : null;

  return {
    layout: pruned,
    activeGroupId: nextActiveGroupId,
    maximizedGroupId: nextMaximized,
    groups: nextGroups
  };
};

export interface SplitGroupOpts {
  tileId?: string;
  groupId?: string;
  newGroupId?: string;
  placeFirst?: boolean;
  direction: SplitDirection;
}

export const splitPanelGroup = (state: ProjectPanelState, opts: SplitGroupOpts): ProjectPanelState => {
  const sourceId = opts.groupId ?? state.activeGroupId;
  const sourceGroup = state.groups[sourceId];
  if (!sourceGroup) return state;

  const createdId = opts.newGroupId ?? newGroupId();
  const moveTileId =
    opts.tileId ?? (sourceGroup.tileIds.length > 1 ? (sourceGroup.activeTileId ?? sourceGroup.tileIds[0]) : undefined);

  const nextGroups: Record<string, PanelGroup> = {};
  for (const [id, group] of Object.entries(state.groups)) {
    if (moveTileId && group.tileIds.includes(moveTileId)) {
      const idx = group.tileIds.indexOf(moveTileId);
      const filtered = group.tileIds.filter((t) => t !== moveTileId);
      const fallback = filtered[Math.min(idx, filtered.length - 1)] ?? null;
      nextGroups[id] = {
        ...group,
        tileIds: filtered,
        activeTileId: group.activeTileId === moveTileId ? fallback : group.activeTileId
      };
    } else {
      nextGroups[id] = group;
    }
  }

  nextGroups[createdId] = {
    id: createdId,
    tileIds: moveTileId ? [moveTileId] : [],
    activeTileId: moveTileId ?? null
  };

  const sourceLeaf: PanelLayoutNode = { type: 'leaf', groupId: sourceId };
  const createdLeaf: PanelLayoutNode = { type: 'leaf', groupId: createdId };
  const splitNode: PanelLayoutNode = {
    type: 'split',
    ratio: 0.5,
    direction: opts.direction,
    first: opts.placeFirst ? createdLeaf : sourceLeaf,
    second: opts.placeFirst ? sourceLeaf : createdLeaf
  };

  const nextLayout = replaceLeafWithSplit(state.layout, sourceId, splitNode);

  return {
    ...state,
    layout: nextLayout,
    activeGroupId: createdId,
    maximizedGroupId: null,
    groups: nextGroups
  };
};

export interface MoveTileOpts {
  index?: number;
  tileId: string;
  zone?: DropZone;
  targetGroupId: string;
}

export const moveTileBetweenGroups = (state: ProjectPanelState, opts: MoveTileOpts): ProjectPanelState => {
  const { tileId, targetGroupId, zone = 'center', index } = opts;
  const targetGroup = state.groups[targetGroupId];
  if (!targetGroup) return state;

  const sourceGroup = Object.values(state.groups).find((g) => g.tileIds.includes(tileId));

  if (zone !== 'center') {
    if (
      sourceGroup &&
      sourceGroup.id === targetGroupId &&
      sourceGroup.tileIds.length === 1 &&
      sourceGroup.tileIds[0] === tileId
    ) {
      return activateTileInProject(state, { tileId, groupId: targetGroupId });
    }

    const direction: SplitDirection = zone === 'left' || zone === 'right' ? 'horizontal' : 'vertical';
    const placeFirst = zone === 'left' || zone === 'top';
    const afterSplit = splitPanelGroup(state, {
      tileId,
      placeFirst,
      direction,
      groupId: targetGroupId
    });

    if (sourceGroup && sourceGroup.id !== targetGroupId) {
      const updatedSource = afterSplit.groups[sourceGroup.id];
      if (updatedSource && updatedSource.tileIds.length === 0) {
        return closePanelGroup(afterSplit, sourceGroup.id);
      }
    }
    return afterSplit;
  }

  const nextGroups: Record<string, PanelGroup> = { ...state.groups };
  if (sourceGroup) {
    const srcIdx = sourceGroup.tileIds.indexOf(tileId);
    const filtered = sourceGroup.tileIds.filter((id) => id !== tileId);
    const fallback = filtered[Math.min(srcIdx, filtered.length - 1)] ?? null;
    nextGroups[sourceGroup.id] = {
      ...sourceGroup,
      tileIds: filtered,
      activeTileId: sourceGroup.activeTileId === tileId ? fallback : sourceGroup.activeTileId
    };
  }

  const baseTarget = nextGroups[targetGroupId]!;
  const withoutTile = baseTarget.tileIds.filter((id) => id !== tileId);
  const insertAt = typeof index === 'number' ? Math.max(0, Math.min(index, withoutTile.length)) : withoutTile.length;
  const nextTileIds = [...withoutTile.slice(0, insertAt), tileId, ...withoutTile.slice(insertAt)];

  nextGroups[targetGroupId] = {
    ...baseTarget,
    tileIds: nextTileIds,
    activeTileId: tileId
  };

  let nextState: ProjectPanelState = {
    ...state,
    activeGroupId: targetGroupId,
    groups: nextGroups
  };

  if (sourceGroup && sourceGroup.id !== targetGroupId && nextGroups[sourceGroup.id]!.tileIds.length === 0) {
    nextState = closePanelGroup(nextState, sourceGroup.id);
  }

  return nextState;
};

export const removeTileFromPanelState = (state: ProjectPanelState, tileId: string): ProjectPanelState => {
  const owner = Object.values(state.groups).find((g) => g.tileIds.includes(tileId));
  if (!owner) return state;

  const idx = owner.tileIds.indexOf(tileId);
  const filtered = owner.tileIds.filter((id) => id !== tileId);
  const nextActiveTile =
    owner.activeTileId === tileId ? (filtered[Math.min(idx, filtered.length - 1)] ?? null) : owner.activeTileId;

  const nextGroups: Record<string, PanelGroup> = {
    ...state.groups,
    [owner.id]: {
      ...owner,
      tileIds: filtered,
      activeTileId: nextActiveTile
    }
  };

  const nextState: ProjectPanelState = {
    ...state,
    groups: nextGroups
  };

  if (filtered.length === 0 && Object.keys(nextGroups).length > 1) {
    return closePanelGroup(nextState, owner.id);
  }

  return nextState;
};

export const updateSplitRatio = (node: PanelLayoutNode, path: ('first' | 'second')[], ratio: number): PanelLayoutNode => {
  if (node.type === 'leaf') return node;
  if (path.length === 0) {
    const clamped = Math.min(MAX_SPLIT_RATIO, Math.max(MIN_SPLIT_RATIO, ratio));
    return clamped === node.ratio ? node : { ...node, ratio: clamped };
  }
  const [head, ...rest] = path;
  if (head === 'first') {
    const first = updateSplitRatio(node.first, rest, ratio);
    return first === node.first ? node : { ...node, first };
  }
  const second = updateSplitRatio(node.second, rest, ratio);
  return second === node.second ? node : { ...node, second };
};

const syncPanelStateForProject = (existing: ProjectPanelState | undefined, projectTiles: Tile[]): ProjectPanelState => {
  const openTiles = projectTiles.filter((t) => !t.tabClosed);
  const validTileIds = new Set(projectTiles.map((t) => t.id));
  let state = existing ?? emptyProjectPanelState(openTiles.map((t) => t.id));

  if (existing) {
    const currentTilesInState = Object.values(state.groups).flatMap((g) => g.tileIds);
    for (const tid of currentTilesInState) {
      if (!validTileIds.has(tid)) {
        state = removeTileFromPanelState(state, tid);
      }
    }
    const afterRemovalSet = new Set(Object.values(state.groups).flatMap((g) => g.tileIds));
    for (const tile of openTiles) {
      if (!afterRemovalSet.has(tile.id)) {
        const targetGroupId = state.groups[state.activeGroupId] ? state.activeGroupId : collectGroupIds(state.layout)[0]!;
        const grp = state.groups[targetGroupId]!;
        const nextIds = [...grp.tileIds, tile.id];
        state = {
          ...state,
          groups: {
            ...state.groups,
            [targetGroupId]: {
              ...grp,
              tileIds: nextIds,
              activeTileId: grp.activeTileId ?? tile.id
            }
          }
        };
      }
    }
  }

  return state;
};

export const syncProjectViewWithTiles = (
  view: ProjectViewState | undefined,
  tiles: Tile[],
  tileKey?: (tile: Tile) => string | null
): ProjectViewState => {
  const knownProjects: string[] = [];
  const seen = new Set<string>();

  const addProject = (raw: string | undefined) => {
    if (!raw) return;
    const norm = projectRootPath(raw);
    if (!norm) return;
    const key = norm.toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    knownProjects.push(norm);
  };

  for (const p of view?.projects ?? []) addProject(p);
  for (const t of tiles) {
    if (t.workspacePath) addProject(t.workspacePath);
  }
  if (knownProjects.length === 0) {
    for (const t of tiles) {
      addProject(t.folderPath ?? t.cwd);
    }
  }
  if (knownProjects.length === 0 && tiles.length > 0) {
    addProject(DEFAULT_PROJECT);
  }
  if (knownProjects.length > 1 && seen.has(DEFAULT_PROJECT)) {
    const realProjects = knownProjects.filter((p) => p !== DEFAULT_PROJECT);
    if (
      !tiles.some(
        (t) =>
          tileProject(t, knownProjects) === DEFAULT_PROJECT && (t.workspacePath === DEFAULT_PROJECT || t.cwd === DEFAULT_PROJECT)
      )
    ) {
      knownProjects.splice(0, knownProjects.length, ...realProjects);
    }
  }

  const prevActiveNorm = view?.activeProject ? normalizeProjectPath(view.activeProject) : null;
  const matchedActive = prevActiveNorm
    ? (knownProjects.find((p) => p.toLowerCase() === prevActiveNorm.toLowerCase()) ?? null)
    : null;
  const activeProject = matchedActive ?? knownProjects[0] ?? null;

  const byProject: Record<string, ProjectPanelState> = {};
  for (const proj of knownProjects) {
    const existingEntry =
      view?.byProject[proj] ??
      Object.entries(view?.byProject ?? {}).find(([k]) => normalizeProjectPath(k).toLowerCase() === proj.toLowerCase())?.[1];
    const matchingTiles = tileKey
      ? tiles.filter((t) => (tileKey(t) ?? tileProject(t, knownProjects))?.toLowerCase() === proj.toLowerCase())
      : tiles.filter((t) => tileMatchesProject(t, proj, knownProjects));
    byProject[proj] = syncPanelStateForProject(existingEntry, matchingTiles);
  }
  if (tiles.length > 0) byProject[GLOBAL_PANEL] = syncPanelStateForProject(view?.byProject[GLOBAL_PANEL], tiles);

  return {
    leftOpen: view?.leftOpen,
    rightOpen: view?.rightOpen,
    leftWidth: view?.leftWidth,
    rightWidth: view?.rightWidth,
    projects: knownProjects,
    activeProject,
    byProject
  };
};
