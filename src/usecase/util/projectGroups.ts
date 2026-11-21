import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';

import { frameOf } from '~/usecase/util/frame';
import { normalizeProjectPath, projectName, tileProject, tileCandidatePath } from '~/usecase/util/projectLayout';

interface BuildProjectGroupsArgs {
  frames: Frame[];
  tiles: Tile[];
  projects: string[];
}

export const dominantProject = (tiles: Tile[], projects: string[]): string | null => {
  const freq = new Map<string, number>();
  for (const tile of tiles) {
    const proj = tileProject(tile, projects);
    if (proj !== null) freq.set(proj, (freq.get(proj) ?? 0) + 1);
  }
  let best: string | null = null;
  let bestCount = 0;
  for (const [proj, count] of freq) {
    if (count > bestCount) {
      best = proj;
      bestCount = count;
    }
  }
  return best;
};

export const buildProjectGroups = ({ frames, tiles, projects }: BuildProjectGroupsArgs): ProjectGroup[] => {
  const frameGroups: ProjectGroup[] = [];
  const frameGroupPathIndex = new Map<string, ProjectGroup>();
  const framedProjects = new Set<string>();

  for (const frame of frames) {
    const memberTiles = tiles.filter((t) => frameOf(frames, t)?.id === frame.id);
    for (const tile of memberTiles) {
      const proj = tileProject(tile, projects);
      if (proj) framedProjects.add(proj.toLowerCase());
    }

    const path = frame.path ? normalizeProjectPath(frame.path) || null : dominantProject(memberTiles, projects);

    const drifted: string[] = [];
    if (path !== null) {
      const normPathLower = path.toLowerCase();
      for (const tile of memberTiles) {
        if (!tileCandidatePath(tile)) continue;
        const proj = tileProject(tile, projects);
        if (proj === null || proj.toLowerCase() !== normPathLower) drifted.push(tile.id);
      }
    }

    const title = frame.title || (path ? projectName(path) : '') || 'Frame';

    const group: ProjectGroup = {
      key: `frame:${frame.id}`,
      kind: 'frame',
      title,
      path,
      frameId: frame.id,
      tiles: memberTiles,
      outside: [],
      drifted
    };
    frameGroups.push(group);

    if (path !== null) {
      const key = path.toLowerCase();
      if (!frameGroupPathIndex.has(key)) frameGroupPathIndex.set(key, group);
    }
  }

  const pathGroups = new Map<string, ProjectGroup>();
  const pathGroupOrder: string[] = [];
  let looseGroup: ProjectGroup | null = null;

  for (const tile of tiles) {
    if (frameOf(frames, tile) !== null) continue;
    const proj = tileProject(tile, projects);

    if (proj !== null) {
      const projLower = proj.toLowerCase();
      const frameGroup = frameGroupPathIndex.get(projLower);
      if (frameGroup) {
        frameGroup.outside.push(tile);
        continue;
      }
      if (!pathGroups.has(projLower)) {
        pathGroups.set(projLower, {
          key: `path:${proj}`,
          kind: 'path',
          title: projectName(proj),
          path: proj,
          frameId: null,
          tiles: [],
          outside: [],
          drifted: []
        });
        pathGroupOrder.push(projLower);
      }
      pathGroups.get(projLower)!.tiles.push(tile);
    } else {
      if (!looseGroup) {
        looseGroup = { key: 'loose', kind: 'loose', title: 'Loose', path: null, frameId: null, tiles: [], outside: [], drifted: [] };
      }
      looseGroup.tiles.push(tile);
    }
  }

  for (const proj of projects) {
    const norm = normalizeProjectPath(proj);
    if (!norm) continue;
    const normLower = norm.toLowerCase();
    if (frameGroupPathIndex.has(normLower) || framedProjects.has(normLower)) continue;
    if (!pathGroups.has(normLower)) {
      pathGroups.set(normLower, {
        key: `path:${norm}`,
        kind: 'path',
        title: projectName(norm),
        path: norm,
        frameId: null,
        tiles: [],
        outside: [],
        drifted: []
      });
    }
  }

  const result: ProjectGroup[] = [...frameGroups];
  const emittedPaths = new Set<string>();

  for (const proj of projects) {
    const normLower = normalizeProjectPath(proj).toLowerCase();
    if (!normLower || emittedPaths.has(normLower)) continue;
    const pg = pathGroups.get(normLower);
    if (pg) {
      result.push(pg);
      emittedPaths.add(normLower);
    }
  }

  for (const projLower of pathGroupOrder) {
    if (emittedPaths.has(projLower)) continue;
    const pg = pathGroups.get(projLower);
    if (pg) {
      result.push(pg);
      emittedPaths.add(projLower);
    }
  }

  if (looseGroup) result.push(looseGroup);

  return result;
};

export const projectsInUse = ({ frames, tiles, projects }: BuildProjectGroupsArgs): string[] =>
  projects.filter((project) => {
    const key = normalizeProjectPath(project).toLowerCase();
    if (!key) return false;
    if (frames.some((f) => f.path && normalizeProjectPath(f.path).toLowerCase() === key)) return true;
    return tiles.some((t) => tileProject(t, projects)?.toLowerCase() === key);
  });

export const groupPathResolver = (groups: ProjectGroup[]): ((tile: Tile) => string | null) => {
  const pathByTile = new Map<string, string>();
  for (const group of groups) {
    if (group.kind !== 'frame' || !group.path) continue;
    for (const tile of [...group.tiles, ...group.outside]) pathByTile.set(tile.id, group.path);
  }
  return (tile) => pathByTile.get(tile.id) ?? null;
};

export const tileOwnerResolver = (args: BuildProjectGroupsArgs): ((tile: Tile) => string | null) => {
  const fromGroups = groupPathResolver(buildProjectGroups(args));
  return (tile) => fromGroups(tile) ?? tileProject(tile, args.projects);
};
