import type { Tile } from '~/domain/interfaces/canvas.interface';
import { parseFrontTitle } from '~/usecase/util/noteMeta';

const ADJ_GAP = 24;

export const adjacentTerm = (note: Tile, tiles: Tile[]): Tile | null => {
  let best: Tile | null = null;
  let bestScore = Infinity;
  for (const t of tiles) {
    if (t.type !== 'term' || t.id === note.id) continue;
    const dx = Math.max(note.x - (t.x + t.width), t.x - (note.x + note.width));
    const dy = Math.max(note.y - (t.y + t.height), t.y - (note.y + note.height));
    const score = Math.max(dx, dy);
    if (score <= ADJ_GAP && score < bestScore) {
      best = t;
      bestScore = score;
    }
  }
  return best;
};

export const termName = (tile: Tile): string => {
  const named = tile.userTitle?.trim();
  if (named) return named;
  const folder = tile.cwd?.replace(/[\\/]+$/, '').split(/[\\/]/).pop();
  return folder || tile.autoTitle || 'Terminal';
};

export const noteLinkTitle = (tile: Tile): string => parseFrontTitle(tile.content) || 'Note';

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

const OFFSET = 40;

export const flowPath = (from: Rect, to: Rect): string => {
  const acx = from.x + from.width / 2;
  const acy = from.y + from.height / 2;
  const bcx = to.x + to.width / 2;
  const bcy = to.y + to.height / 2;
  const dx = bcx - acx;
  const dy = bcy - acy;

  let p1x: number, p1y: number, p2x: number, p2y: number;
  let c1x: number, c1y: number, c2x: number, c2y: number;

  if (Math.abs(dx) >= Math.abs(dy)) {
    const right = dx >= 0;
    p1x = right ? from.x + from.width : from.x;
    p1y = acy;
    p2x = right ? to.x : to.x + to.width;
    p2y = bcy;
    const off = Math.min(OFFSET, Math.abs(p2x - p1x) / 2) || OFFSET;
    c1x = p1x + (right ? off : -off);
    c1y = p1y;
    c2x = p2x + (right ? -off : off);
    c2y = p2y;
  } else {
    const down = dy >= 0;
    p1x = acx;
    p1y = down ? from.y + from.height : from.y;
    p2x = bcx;
    p2y = down ? to.y : to.y + to.height;
    const off = Math.min(OFFSET, Math.abs(p2y - p1y) / 2) || OFFSET;
    c1x = p1x;
    c1y = p1y + (down ? off : -off);
    c2x = p2x;
    c2y = p2y + (down ? -off : off);
  }

  return `M ${p1x} ${p1y} C ${c1x} ${c1y} ${c2x} ${c2y} ${p2x} ${p2y}`;
};
