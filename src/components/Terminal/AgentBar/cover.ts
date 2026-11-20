import { TERM_ROW_PX } from '~/usecase/util/termGrid';

export { TERM_ROW_PX };

const TERM_TOP_INSET_PX = 4;
const TERM_BOTTOM_INSET_PX = 6;
const BAR_BOTTOM_INSET_PX = 4;

export const coverLayout = (
  tileHeight: number,
  naturalHeight: number,
  rows: number,
  inputTop: number | null
): { shiftPx: number; minHeightPx: number | null } => {
  if (inputTop === null || rows <= 0 || tileHeight <= 0) return { shiftPx: 0, minHeightPx: null };
  const top = Math.max(0, Math.min(inputTop, rows - 1));
  const gridTop = Math.max(TERM_TOP_INSET_PX, tileHeight - TERM_BOTTOM_INSET_PX - rows * TERM_ROW_PX);
  const baseHeight = tileHeight - gridTop - BAR_BOTTOM_INSET_PX - top * TERM_ROW_PX;
  const shiftPx = Math.min(top * TERM_ROW_PX, Math.max(0, Math.ceil(naturalHeight - baseHeight)));
  return { shiftPx, minHeightPx: baseHeight + shiftPx };
};
