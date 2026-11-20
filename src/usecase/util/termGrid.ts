import { TILE_MIN_WIDTH, TILE_MIN_HEIGHT } from '~/usecase/util/constants';

export const TERM_ROW_PX = 15;
// measured tile chrome around the terminal box
export const TERM_INSET_W = 16;
export const TERM_INSET_H = 46;

export const termGridOf = (width: number, height: number, cellW: number): { cols: number; rows: number } => ({
  cols: Math.max(20, Math.floor((width - TERM_INSET_W) / cellW)),
  rows: Math.max(2, Math.floor((height - TERM_INSET_H) / TERM_ROW_PX))
});

export const snapTermSize = (width: number, height: number, cellW: number): { width: number; height: number } => {
  let cols = Math.max(20, Math.round((width - TERM_INSET_W) / cellW));
  let rows = Math.max(2, Math.round((height - TERM_INSET_H) / TERM_ROW_PX));
  if (TERM_INSET_W + cols * cellW < TILE_MIN_WIDTH) cols = Math.ceil((TILE_MIN_WIDTH - TERM_INSET_W) / cellW);
  if (TERM_INSET_H + rows * TERM_ROW_PX < TILE_MIN_HEIGHT) rows = Math.ceil((TILE_MIN_HEIGHT - TERM_INSET_H) / TERM_ROW_PX);
  return { width: TERM_INSET_W + cols * cellW, height: TERM_INSET_H + rows * TERM_ROW_PX };
};

// project panel chrome from the panel edge to the terminal box
export const PANEL_CHROME_W = 10;
export const PANEL_CHROME_H = 48;
export const PANEL_TOOLBAR_H = 28;

const SPLIT_DIVIDER_PX = 8;

export const fitCells = (size: number, chrome: number, step: number): number =>
  Math.floor((size - chrome) / step);

export const snapCells = (size: number, chrome: number, step: number): number => {
  const cells = Math.max(2, Math.round((size - chrome) / step));
  return chrome + cells * step;
};

export interface LayoutSplitResult {
  first: number;
  second: number;
  leftover: number;
}

export interface LayoutSplitOpts {
  ratio: number;
  available: number;
  firstChrome: number;
  secondChrome: number;
  step: number;
}

// the second pane floors its remainder so the sub-cell residual ends up after it, outside both panes
export const layoutSplit = ({ ratio, available, firstChrome, secondChrome, step }: LayoutSplitOpts): LayoutSplitResult => {
  const totalContent = available - SPLIT_DIVIDER_PX;
  const snappedFirst = snapCells(ratio * totalContent, firstChrome, step);
  const remainder = totalContent - snappedFirst;
  const cells2 = Math.max(2, Math.floor((remainder - secondChrome) / step));
  const snappedSecond = secondChrome + cells2 * step;
  return { first: snappedFirst, second: snappedSecond, leftover: totalContent - snappedFirst - snappedSecond };
};
