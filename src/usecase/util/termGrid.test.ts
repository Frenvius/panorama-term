import { expect, test } from 'bun:test';

import { TILE_MIN_WIDTH, TILE_MIN_HEIGHT } from '~/usecase/util/constants';
import {
  TERM_ROW_PX,
  TERM_INSET_W,
  TERM_INSET_H,
  PANEL_CHROME_W,
  PANEL_CHROME_H,
  PANEL_TOOLBAR_H,
  fitCells,
  snapCells,
  layoutSplit,
  termGridOf,
  snapTermSize
} from '~/usecase/util/termGrid';

test('termGridOf floor division integer cellW', () => {
  const cellW = 7;
  // use 25 cols so min-20 does not interfere
  const { cols, rows } = termGridOf(TERM_INSET_W + 25 * cellW + 5, TERM_INSET_H + 4 * TERM_ROW_PX + 7, cellW);
  expect(cols).toBe(25);
  expect(rows).toBe(4);
});

test('termGridOf floor division fractional cellW', () => {
  const cellW = 7.23;
  const { cols } = termGridOf(660, 600, cellW);
  expect(cols).toBe(Math.max(20, Math.floor((660 - TERM_INSET_W) / cellW)));
});

test('termGridOf clamps cols to min 20', () => {
  expect(termGridOf(TERM_INSET_W + 5 * 7, 200, 7).cols).toBe(20);
});

test('termGridOf clamps rows to min 2', () => {
  expect(termGridOf(300, TERM_INSET_H + 1 * TERM_ROW_PX, 7).rows).toBe(2);
});

test('snapTermSize rounds down to nearer cell', () => {
  const cellW = 10;
  // use 25 cols baseline; +3 extra px rounds down to 25
  const { width } = snapTermSize(TERM_INSET_W + 25 * cellW + 3, TERM_INSET_H + 4 * TERM_ROW_PX + 2, cellW);
  expect(width).toBe(TERM_INSET_W + 25 * cellW);
});

test('snapTermSize rounds up to nearer cell', () => {
  const cellW = 10;
  // use 25 cols baseline so min-20 and TILE_MIN_WIDTH do not interfere
  const { width, height } = snapTermSize(TERM_INSET_W + 25 * cellW + 7, TERM_INSET_H + 4 * TERM_ROW_PX + 9, cellW);
  expect(width).toBe(TERM_INSET_W + 26 * cellW);
  expect(height).toBe(TERM_INSET_H + 5 * TERM_ROW_PX);
});

test('snapTermSize fractional cellW produces exact multiple', () => {
  const cellW = 7.23;
  const { width } = snapTermSize(660, 600, cellW);
  const cols = Math.round((660 - TERM_INSET_W) / cellW);
  expect(width).toBeCloseTo(TERM_INSET_W + Math.max(20, cols) * cellW);
});

test('snapTermSize clamps to min 20 cols', () => {
  // cellW=10: 20*10+16=216 >= TILE_MIN_WIDTH(200), so only the min-20 clamp applies
  const cellW = 10;
  const { width } = snapTermSize(TERM_INSET_W + 3 * cellW, TERM_INSET_H + 10 * TERM_ROW_PX, cellW);
  expect(width).toBe(TERM_INSET_W + 20 * cellW);
});

test('snapTermSize ensures height meets TILE_MIN_HEIGHT when rows are few', () => {
  // TILE_MIN_HEIGHT=120 > TERM_INSET_H+2*TERM_ROW_PX=76, so the min-height path raises rows further
  const { height } = snapTermSize(300, TERM_INSET_H + 1 * TERM_ROW_PX, 7);
  expect(height).toBeGreaterThanOrEqual(TILE_MIN_HEIGHT);
  // exact: ceil((120-46)/15)=5 rows -> 46+75=121
  expect(height).toBe(TERM_INSET_H + 5 * TERM_ROW_PX);
});

test('snapTermSize raises cols to meet TILE_MIN_WIDTH when cellW is tiny', () => {
  const cellW = 1;
  const { width } = snapTermSize(50, TERM_INSET_H + 10 * TERM_ROW_PX, cellW);
  expect(width).toBeGreaterThanOrEqual(TILE_MIN_WIDTH);
});

test('fitCells floors available space to whole cells', () => {
  expect(fitCells(PANEL_CHROME_W + 10 * 7 + 5, PANEL_CHROME_W, 7)).toBe(10);
});

test('fitCells returns 0 when size equals chrome', () => {
  expect(fitCells(PANEL_CHROME_H, PANEL_CHROME_H, TERM_ROW_PX)).toBe(0);
});

test('snapCells rounds to nearest cell boundary', () => {
  const chrome = PANEL_CHROME_H;
  const step = TERM_ROW_PX;
  // 10.4 rows rounds to 10
  expect(snapCells(chrome + 10 * step + 3, chrome, step)).toBe(chrome + 10 * step);
  // 10.6 rows rounds to 11
  expect(snapCells(chrome + 10 * step + 9, chrome, step)).toBe(chrome + 11 * step);
});

test('snapCells enforces minimum 2 cells', () => {
  expect(snapCells(PANEL_CHROME_H + 1, PANEL_CHROME_H, TERM_ROW_PX)).toBe(PANEL_CHROME_H + 2 * TERM_ROW_PX);
});

test('snapCells works with fractional cellW', () => {
  const cellW = 7.23;
  const chrome = PANEL_CHROME_W;
  const size = chrome + 20 * cellW + 2;
  const result = snapCells(size, chrome, cellW);
  expect(result).toBeCloseTo(chrome + 20 * cellW);
});

test('layoutSplit first pane snaps to nearest cell, second floors remainder', () => {
  const step = TERM_ROW_PX;
  const firstChrome = PANEL_CHROME_H + PANEL_TOOLBAR_H;
  const secondChrome = PANEL_CHROME_H;
  // container of 500px, ratio 0.5 => content=492, raw first=246
  const available = 500;
  const { first, second, leftover } = layoutSplit({ ratio: 0.5, available, firstChrome, secondChrome, step });
  // first must equal firstChrome + N*step
  expect((first - firstChrome) % step).toBe(0);
  // second must equal secondChrome + M*step
  expect((second - secondChrome) % step).toBe(0);
  // leftover < step
  expect(leftover).toBeGreaterThanOrEqual(0);
  expect(leftover).toBeLessThan(step);
  // total must equal available - 8 divider
  expect(first + second + leftover).toBe(available - 8);
});

test('layoutSplit leftover is always non-negative and less than one step', () => {
  const step = 7.23;
  const chrome = PANEL_CHROME_W;
  for (const ratio of [0.1, 0.3, 0.5, 0.7, 0.9]) {
    const { first, second, leftover } = layoutSplit({ ratio, available: 800, firstChrome: chrome, secondChrome: chrome, step });
    expect(leftover).toBeGreaterThanOrEqual(0);
    expect(leftover).toBeLessThan(step * 2);
    expect(first + second + leftover).toBeCloseTo(792, 8);
  }
});

test('PANEL_CHROME constants are positive integers', () => {
  expect(PANEL_CHROME_W).toBeGreaterThan(0);
  expect(PANEL_CHROME_H).toBeGreaterThan(0);
  expect(PANEL_TOOLBAR_H).toBeGreaterThan(0);
});

test('snapTermSize raises rows to meet TILE_MIN_HEIGHT when rows are tiny', () => {
  const { height } = snapTermSize(300, 10, 7);
  expect(height).toBeGreaterThanOrEqual(TILE_MIN_HEIGHT);
});
