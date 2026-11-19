export const TERM_ROW_PX = 15;
const TERM_TOP_INSET_PX = 4;
const BAR_BOTTOM_INSET_PX = 4;

export const coverLayout = (
  tileHeight: number,
  naturalHeight: number,
  rows: number,
  inputTop: number | null
): { shiftRows: number; minHeightPx: number | null } => {
  if (inputTop === null || rows <= 0 || tileHeight <= 0) return { shiftRows: 0, minHeightPx: null };
  const top = Math.max(0, Math.min(inputTop, rows - 1));
  const baseHeight = tileHeight - TERM_TOP_INSET_PX - BAR_BOTTOM_INSET_PX - top * TERM_ROW_PX;
  const shiftRows = Math.min(top, Math.max(0, Math.ceil((naturalHeight - baseHeight) / TERM_ROW_PX)));
  return { shiftRows, minHeightPx: baseHeight + shiftRows * TERM_ROW_PX };
};
