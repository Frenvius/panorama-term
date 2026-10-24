import { describe, expect, it } from 'bun:test';

import { insideSafeTriangle } from '~/usecase/util/submenuAim';

const flyout = { left: 0, right: 100, top: 0, bottom: 200 };
const origin = { x: 140, y: 190 };

describe('insideSafeTriangle', () => {
  it('keeps a diagonal path toward a submenu on the left', () => {
    expect(insideSafeTriangle(origin, { x: 120, y: 150 }, flyout)).toBe(true);
    expect(insideSafeTriangle(origin, { x: 105, y: 60 }, flyout)).toBe(true);
  });

  it('drops a path moving away from the submenu', () => {
    expect(insideSafeTriangle(origin, { x: 160, y: 150 }, flyout)).toBe(false);
    expect(insideSafeTriangle(origin, { x: 130, y: 230 }, flyout)).toBe(false);
  });

  it('mirrors for a submenu on the right', () => {
    const right = { left: 200, right: 300, top: 0, bottom: 200 };
    expect(insideSafeTriangle({ x: 160, y: 190 }, { x: 195, y: 60 }, right)).toBe(true);
    expect(insideSafeTriangle({ x: 160, y: 190 }, { x: 140, y: 100 }, right)).toBe(false);
  });
});
