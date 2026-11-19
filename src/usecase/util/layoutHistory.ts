import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Layout {
  tiles: Tile[];
  frames: Frame[];
}

export interface LayoutSnapshot {
  frames: Frame[];
  tiles: Map<string, Rect>;
}

const FRAME_KEYS: (keyof Frame)[] = ['id', 'x', 'y', 'width', 'height', 'title', 'color', 'path'];

const sameRect = (a: Rect, b: Rect): boolean => a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;

const sameFrames = (a: Frame[], b: Frame[]): boolean =>
  a === b || (a.length === b.length && a.every((f, i) => FRAME_KEYS.every((k) => f[k] === b[i]![k])));

export const captureLayout = ({ tiles, frames }: Layout): LayoutSnapshot => ({
  frames,
  tiles: new Map(tiles.map((t) => [t.id, { x: t.x, y: t.y, width: t.width, height: t.height }]))
});

export const layoutChanged = (from: LayoutSnapshot, to: LayoutSnapshot): boolean => {
  if (!sameFrames(from.frames, to.frames)) return true;
  for (const [id, rect] of to.tiles) {
    const prev = from.tiles.get(id);
    if (prev && !sameRect(prev, rect)) return true;
  }
  return false;
};

export const restoreLayout = (snapshot: LayoutSnapshot, current: Layout): Layout | null => {
  const tiles = current.tiles.map((t) => {
    const rect = snapshot.tiles.get(t.id);
    return rect && !sameRect(rect, t) ? { ...t, ...rect } : t;
  });
  const next = { tiles, frames: snapshot.frames };
  return layoutChanged(captureLayout(current), captureLayout(next)) ? next : null;
};
