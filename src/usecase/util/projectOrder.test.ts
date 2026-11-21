import type { Frame } from '~/domain/interfaces/workspace.interface';

import { test, expect, describe } from 'bun:test';

import { moveProject, moveFrameOrder } from '~/usecase/util/projectOrder';

describe('moveProject', () => {
  const projects = ['/a', '/b', '/c', '/d'];

  test('moves an item before a later target', () => {
    expect(moveProject({ projects, from: '/d', to: '/b', place: 'before' })).toEqual(['/a', '/d', '/b', '/c']);
  });

  test('moves an item after a later target', () => {
    expect(moveProject({ projects, from: '/a', to: '/c', place: 'after' })).toEqual(['/b', '/c', '/a', '/d']);
  });

  test('moves an item up before an earlier target', () => {
    expect(moveProject({ projects, from: '/c', to: '/a', place: 'before' })).toEqual(['/c', '/a', '/b', '/d']);
  });

  test('moves an item down after the last target', () => {
    expect(moveProject({ projects, from: '/a', to: '/d', place: 'after' })).toEqual(['/b', '/c', '/d', '/a']);
  });

  test('is a no-op when dropped onto itself', () => {
    expect(moveProject({ projects, from: '/b', to: '/b', place: 'after' })).toEqual(projects);
  });

  test('is a no-op when the source or target is unknown', () => {
    expect(moveProject({ projects, from: '/x', to: '/b', place: 'before' })).toEqual(projects);
    expect(moveProject({ projects, from: '/b', to: '/x', place: 'before' })).toEqual(projects);
  });

  test('does not mutate the input array', () => {
    const input = [...projects];
    moveProject({ projects: input, from: '/d', to: '/a', place: 'before' });
    expect(input).toEqual(projects);
  });
});

describe('moveFrameOrder', () => {
  const frame = (id: string): Frame => ({ id, x: 0, y: 0, width: 100, height: 100, title: id, color: '#000000' });
  const frames = [frame('f1'), frame('f2'), frame('f3')];

  test('reorders frames keeping their objects', () => {
    const next = moveFrameOrder({ frames, from: 'f3', to: 'f1', place: 'before' });
    expect(next.map((f) => f.id)).toEqual(['f3', 'f1', 'f2']);
    expect(next[0]).toBe(frames[2]);
  });

  test('moves a frame after another frame', () => {
    const next = moveFrameOrder({ frames, from: 'f1', to: 'f2', place: 'after' });
    expect(next.map((f) => f.id)).toEqual(['f2', 'f1', 'f3']);
  });
});
