import type { Tile } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import {
  projectName,
  tileProject,
  pathStartsWith,
  closePanelGroup,
  splitPanelGroup,
  collectGroupIds,
  updateSplitRatio,
  tileMatchesProject,
  normalizeProjectPath,
  activateTileInProject,
  moveTileBetweenGroups,
  emptyProjectPanelState,
  syncProjectViewWithTiles,
  removeTileFromPanelState
} from '~/usecase/util/projectLayout';

const makeTile = (id: string, overrides: Partial<Tile> = {}): Tile => ({
  id,
  type: 'term',
  x: 0,
  y: 0,
  width: 400,
  height: 300,
  zIndex: 1,
  ...overrides
});

describe('project path helpers', () => {
  test('normalizeProjectPath normalizes slashes and trailing slashes', () => {
    expect(normalizeProjectPath('D:\\workspace\\panorama\\')).toBe('D:/workspace/panorama');
    expect(normalizeProjectPath('/Users/dev/project///')).toBe('/Users/dev/project');
    expect(normalizeProjectPath('/')).toBe('/');
    expect(normalizeProjectPath('   ')).toBe('');
  });

  test('projectName extracts folder basename', () => {
    expect(projectName('D:\\workspace\\panorama\\')).toBe('panorama');
    expect(projectName('/Users/dev/my-app')).toBe('my-app');
    expect(projectName('/')).toBe('/');
  });

  test('pathStartsWith matches exact or subdirectory boundaries only', () => {
    expect(pathStartsWith('/work/app/src/index.ts', '/work/app')).toBe(true);
    expect(pathStartsWith('D:\\Work\\App\\src', 'd:/work/app')).toBe(true);
    expect(pathStartsWith('/work/app-other', '/work/app')).toBe(false);
  });

  test('tileProject and tileMatchesProject pick longest matching project prefix', () => {
    const projects = ['/work/mono', '/work/mono/packages/ui'];
    const rootTile = makeTile('t1', { cwd: '/work/mono/scripts' });
    const pkgTile = makeTile('t2', { filePath: '/work/mono/packages/ui/Button.tsx' });
    const otherTile = makeTile('t3', { cwd: '/work/other' });

    expect(tileProject(rootTile, projects)).toBe('/work/mono');
    expect(tileProject(pkgTile, projects)).toBe('/work/mono/packages/ui');
    expect(tileProject(otherTile, projects)).toBeNull();

    expect(tileMatchesProject(pkgTile, '/work/mono', projects)).toBe(false);
    expect(tileMatchesProject(pkgTile, '/work/mono/packages/ui', projects)).toBe(true);
  });
});

describe('binary split panel tree operations', () => {
  test('emptyProjectPanelState creates a single leaf group', () => {
    const state = emptyProjectPanelState(['t1', 't2', 't1'], 'g1');
    expect(collectGroupIds(state.layout)).toEqual(['g1']);
    expect(state.activeGroupId).toBe('g1');
    expect(state.groups.g1?.tileIds).toEqual(['t1', 't2']);
    expect(state.groups.g1?.activeTileId).toBe('t1');
  });

  test('activateTileInProject activates existing tile or appends new tile', () => {
    let state = emptyProjectPanelState(['t1', 't2'], 'g1');
    state = activateTileInProject(state, { tileId: 't2' });
    expect(state.groups.g1?.activeTileId).toBe('t2');

    state = activateTileInProject(state, { tileId: 't3' });
    expect(state.groups.g1?.tileIds).toEqual(['t1', 't2', 't3']);
    expect(state.groups.g1?.activeTileId).toBe('t3');
  });

  test('splitPanelGroup moves active tile when group has multiple tiles', () => {
    const initial = emptyProjectPanelState(['t1', 't2'], 'g1');
    const split = splitPanelGroup(initial, {
      groupId: 'g1',
      newGroupId: 'g2',
      direction: 'horizontal'
    });

    expect(collectGroupIds(split.layout)).toEqual(['g1', 'g2']);
    expect(split.activeGroupId).toBe('g2');
    expect(split.groups.g1?.tileIds).toEqual(['t2']);
    expect(split.groups.g1?.activeTileId).toBe('t2');
    expect(split.groups.g2?.tileIds).toEqual(['t1']);
    expect(split.groups.g2?.activeTileId).toBe('t1');
  });

  test('splitPanelGroup creates empty split group when single-tile group is split without explicit tileId', () => {
    const initial = emptyProjectPanelState(['t1'], 'g1');
    const split = splitPanelGroup(initial, {
      groupId: 'g1',
      newGroupId: 'g2',
      direction: 'vertical'
    });

    expect(collectGroupIds(split.layout)).toEqual(['g1', 'g2']);
    expect(split.groups.g1?.tileIds).toEqual(['t1']);
    expect(split.groups.g2?.tileIds).toEqual([]);
  });

  test('moveTileBetweenGroups reorders within center and prunes emptied source group on split drop', () => {
    let state = emptyProjectPanelState(['t1', 't2'], 'g1');
    state = splitPanelGroup(state, {
      groupId: 'g1',
      newGroupId: 'g2',
      direction: 'horizontal'
    });

    const merged = moveTileBetweenGroups(state, {
      tileId: 't1',
      targetGroupId: 'g1',
      zone: 'center',
      index: 0
    });
    expect(collectGroupIds(merged.layout)).toEqual(['g1']);
    expect(merged.groups.g1?.tileIds).toEqual(['t1', 't2']);
    expect(merged.groups.g1?.activeTileId).toBe('t1');

    const resplit = moveTileBetweenGroups(merged, {
      tileId: 't2',
      targetGroupId: 'g1',
      zone: 'bottom'
    });
    expect(collectGroupIds(resplit.layout)).toHaveLength(2);
    expect(resplit.layout.type).toBe('split');
    if (resplit.layout.type === 'split') {
      expect(resplit.layout.direction).toBe('vertical');
    }
  });

  test('removeTileFromPanelState removes tile and collapses empty split group', () => {
    let state = emptyProjectPanelState(['t1', 't2'], 'g1');
    state = splitPanelGroup(state, {
      groupId: 'g1',
      newGroupId: 'g2',
      direction: 'horizontal'
    });

    const afterRemove = removeTileFromPanelState(state, 't1');
    expect(collectGroupIds(afterRemove.layout)).toEqual(['g1']);
    expect(afterRemove.activeGroupId).toBe('g1');
    expect(afterRemove.groups.g1?.tileIds).toEqual(['t2']);

    const afterRemoveLast = removeTileFromPanelState(afterRemove, 't2');
    expect(collectGroupIds(afterRemoveLast.layout)).toEqual(['g1']);
    expect(afterRemoveLast.groups.g1?.tileIds).toEqual([]);
    expect(afterRemoveLast.groups.g1?.activeTileId).toBeNull();
  });

  test('updateSplitRatio clamps and updates nested split node ratio', () => {
    let state = emptyProjectPanelState(['t1', 't2', 't3'], 'g1');
    state = splitPanelGroup(state, { groupId: 'g1', newGroupId: 'g2', direction: 'horizontal' });
    state = splitPanelGroup(state, { groupId: 'g1', newGroupId: 'g3', direction: 'vertical' });

    const updatedRoot = updateSplitRatio(state.layout, [], 0.75);
    expect(updatedRoot.type === 'split' && updatedRoot.ratio).toBe(0.75);

    const updatedNested = updateSplitRatio(updatedRoot, ['first'], 0.02);
    if (updatedNested.type === 'split' && updatedNested.first.type === 'split') {
      expect(updatedNested.first.ratio).toBe(0.1);
    }

    const closed = closePanelGroup({ ...state, layout: updatedNested }, 'g3');
    expect(collectGroupIds(closed.layout)).toEqual(['g1', 'g2']);
  });

  test('syncProjectViewWithTiles discovers projects and syncs tile memberships', () => {
    const tiles = [
      makeTile('t1', { cwd: '/projects/alpha/src' }),
      makeTile('t2', { workspacePath: '/projects/beta', cwd: '/projects/beta' })
    ];
    const synced = syncProjectViewWithTiles(undefined, tiles);
    expect(synced.projects).toEqual(['/projects/beta']);

    const syncedBoth = syncProjectViewWithTiles(
      { projects: ['/projects/alpha', '/projects/beta'], activeProject: '/projects/alpha', byProject: {} },
      tiles
    );
    expect(syncedBoth.projects).toEqual(['/projects/alpha', '/projects/beta']);
    expect(syncedBoth.activeProject).toBe('/projects/alpha');
    const alphaGroups = Object.values(syncedBoth.byProject['/projects/alpha']!.groups);
    expect(alphaGroups.flatMap((g) => g.tileIds)).toEqual(['t1']);

    const unassignedTerm = [makeTile('t3', { type: 'term' })];
    const syncedDefault = syncProjectViewWithTiles(undefined, unassignedTerm);
    expect(syncedDefault.projects).toEqual(['~']);
    expect(syncedDefault.activeProject).toBe('~');
    const defaultGroups = Object.values(syncedDefault.byProject['~']!.groups);
    expect(defaultGroups.flatMap((g) => g.tileIds)).toEqual(['t3']);
  });

  test('syncProjectViewWithTiles uses tileKey resolver when provided', () => {
    const tiles = [
      makeTile('t1', { cwd: '/projects/alpha/src' }),
      makeTile('t2', { cwd: '/projects/beta' })
    ];
    const keyMap = new Map([['t1', '/projects/alpha'], ['t2', '/projects/beta']]);
    const tileKey = (t: Tile): string | null => keyMap.get(t.id) ?? null;
    const view = { projects: ['/projects/alpha', '/projects/beta'], activeProject: '/projects/alpha', byProject: {} };
    const result = syncProjectViewWithTiles(view, tiles, tileKey);
    const alphaIds = Object.values(result.byProject['/projects/alpha']!.groups).flatMap((g) => g.tileIds);
    const betaIds = Object.values(result.byProject['/projects/beta']!.groups).flatMap((g) => g.tileIds);
    expect(alphaIds).toEqual(['t1']);
    expect(betaIds).toEqual(['t2']);
  });

  test('syncProjectViewWithTiles tileKey null falls back to path matching', () => {
    const tiles = [makeTile('t1', { cwd: '/projects/alpha' }), makeTile('t2', {}), makeTile('t3', { cwd: '/projects/alpha/x' })];
    const tileKey = (t: Tile): string | null => (t.id === 't1' ? '/projects/alpha' : null);
    const view = { projects: ['/projects/alpha'], activeProject: '/projects/alpha', byProject: {} };
    const result = syncProjectViewWithTiles(view, tiles, tileKey);
    const alphaIds = Object.values(result.byProject['/projects/alpha']!.groups).flatMap((g) => g.tileIds);
    expect(alphaIds).toEqual(['t1', 't3']);
    expect(alphaIds).not.toContain('t2');
  });
});
