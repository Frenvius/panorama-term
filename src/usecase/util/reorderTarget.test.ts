import { test, expect, describe } from 'bun:test';

import { reorderTargetIndex } from '~/usecase/util/reorderTarget';

const rects = [0, 22, 44, 66].map((top) => ({ top, height: 20 }));

describe('reorderTargetIndex', () => {
  test('moves last to first', () => {
    expect(reorderTargetIndex({ centerY: 5, rects, from: 3 })).toBe(0);
  });

  test('moves first to last', () => {
    expect(reorderTargetIndex({ centerY: 80, rects, from: 0 })).toBe(3);
  });

  test('moves a middle item up', () => {
    expect(reorderTargetIndex({ centerY: 20, rects, from: 2 })).toBe(1);
  });

  test('moves a middle item down', () => {
    expect(reorderTargetIndex({ centerY: 55, rects, from: 1 })).toBe(2);
  });

  test('keeps the slot when no neighbour midpoint is crossed', () => {
    expect(reorderTargetIndex({ centerY: 32, rects, from: 1 })).toBe(1);
  });

  test('two items: second to first', () => {
    expect(reorderTargetIndex({ centerY: 5, rects: rects.slice(0, 2), from: 1 })).toBe(0);
  });

  test('two items: first to second', () => {
    expect(reorderTargetIndex({ centerY: 40, rects: rects.slice(0, 2), from: 0 })).toBe(1);
  });

  test('clamps above the first row to 0', () => {
    expect(reorderTargetIndex({ centerY: -500, rects, from: 2 })).toBe(0);
  });

  test('clamps below the last row to n-1', () => {
    expect(reorderTargetIndex({ centerY: 900, rects, from: 1 })).toBe(3);
  });

  test('reaches first when the first row is shorter than the dragged one', () => {
    const mixed = [{ top: 0, height: 10 }, { top: 12, height: 60 }];
    expect(reorderTargetIndex({ centerY: 4, rects: mixed, from: 1 })).toBe(0);
  });
});
