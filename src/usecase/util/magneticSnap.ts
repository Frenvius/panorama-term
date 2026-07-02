import type { Tile } from '~/domain/interfaces/workspace.interface';

export interface DragSnap {
  x: number | null;
  y: number | null;
}

export const computeDragSnap = (tile: Tile, others: Tile[], threshold: number): DragSnap => {
  const L = tile.x;
  const R = tile.x + tile.width;
  const T = tile.y;
  const B = tile.y + tile.height;

  let bestX: number | null = null;
  let bestXd = threshold;
  let bestY: number | null = null;
  let bestYd = threshold;

  const considerX = (target: number) => {
    const d = Math.abs(target - tile.x);
    if (d < bestXd) {
      bestXd = d;
      bestX = target;
    }
  };
  const considerY = (target: number) => {
    const d = Math.abs(target - tile.y);
    if (d < bestYd) {
      bestYd = d;
      bestY = target;
    }
  };

  for (const o of others) {
    const oL = o.x;
    const oR = o.x + o.width;
    const oT = o.y;
    const oB = o.y + o.height;

    if (T < oB + threshold && B > oT - threshold) {
      considerX(oR);
      considerX(oL - tile.width);
      considerX(oL);
      considerX(oR - tile.width);
    }
    if (L < oR + threshold && R > oL - threshold) {
      considerY(oB);
      considerY(oT - tile.height);
      considerY(oT);
      considerY(oB - tile.height);
    }
  }

  return { x: bestX, y: bestY };
};
