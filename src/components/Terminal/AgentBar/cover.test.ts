import { describe, expect, test } from 'bun:test';
import { coverLayout } from './cover';

describe('coverLayout', () => {
  test('leaves the bar uncovered without an input box', () => {
    expect(coverLayout(308, 100, 20, null)).toEqual({ shiftPx: 0, minHeightPx: null });
  });
  test('aligns the cover and shifts only as many pixels as needed', () => {
    expect(coverLayout(308, 45, 20, 17)).toEqual({ shiftPx: 0, minHeightPx: 45 });
    expect(coverLayout(308, 46, 20, 17)).toEqual({ shiftPx: 1, minHeightPx: 46 });
    expect(coverLayout(308, 76, 20, 17)).toEqual({ shiftPx: 31, minHeightPx: 76 });
    expect(coverLayout(308, 63.2, 20, 17)).toEqual({ shiftPx: 19, minHeightPx: 64 });
  });
  test('accounts for the leftover space above a bottom-anchored grid', () => {
    expect(coverLayout(318, 47, 20, 17)).toEqual({ shiftPx: 0, minHeightPx: 47 });
    expect(coverLayout(318, 48, 20, 17)).toEqual({ shiftPx: 1, minHeightPx: 48 });
  });
  test('clamps the shift to the input row', () => {
    expect(coverLayout(308, 500, 20, 2)).toEqual({ shiftPx: 30, minHeightPx: 300 });
  });
});
