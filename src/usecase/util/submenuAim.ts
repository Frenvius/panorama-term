export interface Point {
  x: number;
  y: number;
}

interface EdgeRect {
  top: number;
  left: number;
  right: number;
  bottom: number;
}

const SLACK = 4;

const cross = (a: Point, b: Point, p: Point): number => (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);

export const insideSafeTriangle = (origin: Point, point: Point, submenu: EdgeRect): boolean => {
  const edgeX = origin.x >= submenu.right ? submenu.right : submenu.left;
  const top = { x: edgeX, y: submenu.top - SLACK };
  const bottom = { x: edgeX, y: submenu.bottom + SLACK };
  const d1 = cross(origin, top, point);
  const d2 = cross(top, bottom, point);
  const d3 = cross(bottom, origin, point);
  const hasNeg = d1 < 0 || d2 < 0 || d3 < 0;
  const hasPos = d1 > 0 || d2 > 0 || d3 > 0;
  return !(hasNeg && hasPos);
};
