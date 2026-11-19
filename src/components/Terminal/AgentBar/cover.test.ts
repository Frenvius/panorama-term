import { describe, expect, test } from 'bun:test';
import { coverLayout } from './cover';

describe('coverLayout', () => {
  test('leaves the bar uncovered without an input box', () => {
    expect(coverLayout(308, 100, 20, null)).toEqual({ shiftRows: 0, minHeightPx: null });
  });
  test('aligns the cover and shifts only as many rows as needed', () => {
    expect(coverLayout(308, 45, 20, 17)).toEqual({ shiftRows: 0, minHeightPx: 45 });
    expect(coverLayout(308, 46, 20, 17)).toEqual({ shiftRows: 1, minHeightPx: 60 });
    expect(coverLayout(308, 76, 20, 17)).toEqual({ shiftRows: 3, minHeightPx: 90 });
  });
  test('clamps the shift to the input row', () => {
    expect(coverLayout(308, 500, 20, 2)).toEqual({ shiftRows: 2, minHeightPx: 300 });
  });
});
