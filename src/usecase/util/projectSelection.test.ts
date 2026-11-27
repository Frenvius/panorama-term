import type { ProjectViewState } from '~/domain/interfaces/projectView.interface';

import { test, expect } from 'bun:test';

import { keepLocalSelection } from '~/usecase/util/projectSelection';

const view = (activeProject: string, activeGroupId: string, activeTileId: string, tileIds = ['a', 'b']): ProjectViewState => ({
  projects: ['p1', 'p2'],
  activeProject,
  byProject: {
    p1: {
      layout: { type: 'leaf', groupId: 'g1' },
      activeGroupId,
      groups: { g1: { id: 'g1', tileIds, activeTileId }, g2: { id: 'g2', tileIds: [], activeTileId: null } }
    }
  }
});

test('keeps local active project, group and tile', () => {
  const result = keepLocalSelection(view('p2', 'g2', 'b'), view('p1', 'g1', 'a'));
  expect(result.activeProject).toBe('p1');
  expect(result.byProject.p1.activeGroupId).toBe('g1');
  expect(result.byProject.p1.groups.g1.activeTileId).toBe('a');
});

test('takes incoming selection when the local one was closed', () => {
  const result = keepLocalSelection(view('p1', 'g1', 'b', ['b']), view('p1', 'g1', 'a'));
  expect(result.byProject.p1.groups.g1.activeTileId).toBe('b');
  expect(result.byProject.p1.groups.g1.tileIds).toEqual(['b']);
});
