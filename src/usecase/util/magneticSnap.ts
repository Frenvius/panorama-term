export interface SnapRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface DragSnap {
  x: number | null;
  y: number | null;
}

export interface ResizeSnap {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const computeResizeSnap = (tile: SnapRect, dir: string, others: SnapRect[], threshold: number, minW: number, minH: number): ResizeSnap => {
  const L = tile.x;
  const R = tile.x + tile.width;
  const T = tile.y;
  const B = tile.y + tile.height;

  let bestX: number | null = null;
  let bestXd = threshold;
  let bestY: number | null = null;
  let bestYd = threshold;

  const xEdge = dir.includes('e') ? R : dir.includes('w') ? L : null;
  const yEdge = dir.includes('s') ? B : dir.includes('n') ? T : null;

  for (const o of others) {
    const oL = o.x;
    const oR = o.x + o.width;
    const oT = o.y;
    const oB = o.y + o.height;

    if (xEdge !== null && T < oB + threshold && B > oT - threshold) {
      for (const target of [oL, oR]) {
        const d = Math.abs(target - xEdge);
        if (d < bestXd) {
          bestXd = d;
          bestX = target;
        }
      }
    }
    if (yEdge !== null && L < oR + threshold && R > oL - threshold) {
      for (const target of [oT, oB]) {
        const d = Math.abs(target - yEdge);
        if (d < bestYd) {
          bestYd = d;
          bestY = target;
        }
      }
    }
  }

  let { x, y, width, height } = tile;

  if (bestX !== null) {
    if (dir.includes('e') && bestX - x >= minW) width = bestX - x;
    else if (dir.includes('w') && R - bestX >= minW) {
      x = bestX;
      width = R - bestX;
    }
  }
  if (bestY !== null) {
    if (dir.includes('s') && bestY - y >= minH) height = bestY - y;
    else if (dir.includes('n') && B - bestY >= minH) {
      y = bestY;
      height = B - bestY;
    }
  }

  return { x, y, width, height };
};

export const computeDragSnap = (tile: SnapRect, others: SnapRect[], threshold: number): DragSnap => {
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
