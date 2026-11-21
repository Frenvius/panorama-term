import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';

export const withFrameCreated = (frames: Frame[], frame: Omit<Frame, 'id'>, id: string): Frame[] =>
  [...frames, { ...frame, id }];

export const withFramePatched = (frames: Frame[], id: string, patch: Partial<Omit<Frame, 'id'>>): Frame[] =>
  frames.map((f) => (f.id === id ? { ...f, ...patch } : f));

export const tileInFrame = (frame: Frame, tile: Pick<Tile, 'x' | 'y' | 'width' | 'height'>): boolean => {
  const cx = tile.x + tile.width / 2;
  const cy = tile.y + tile.height / 2;
  return cx >= frame.x && cx <= frame.x + frame.width && cy >= frame.y && cy <= frame.y + frame.height;
};

export const frameOf = (frames: Frame[], tile: Pick<Tile, 'x' | 'y' | 'width' | 'height'>): Frame | null => {
  let best: Frame | null = null;
  let bestArea = Infinity;
  for (const frame of frames) {
    if (!tileInFrame(frame, tile)) continue;
    const area = frame.width * frame.height;
    if (area <= bestArea) {
      best = frame;
      bestArea = area;
    }
  }
  return best;
};

export const groupByFrame = (frames: Frame[], tiles: Tile[]): { members: Map<string, Tile[]>; loose: Tile[] } => {
  const members = new Map<string, Tile[]>(frames.map((f) => [f.id, [] as Tile[]]));
  const loose: Tile[] = [];

  for (const tile of tiles) {
    const frame = frameOf(frames, tile);
    if (frame) members.get(frame.id)?.push(tile);
    else loose.push(tile);
  }

  return { members, loose };
};
