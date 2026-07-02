import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import { CELL, MAJOR, ZOOM_MIN } from '~/usecase/util/constants';

export const drawGrid = (canvas: HTMLCanvasElement, view: View, tiles: Tile[]): void => {
  const ctx = canvas.getContext('2d');
  if (!ctx) return;
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  if (w === 0 || h === 0) return;

  const dpr = window.devicePixelRatio || 1;
  if (canvas.width !== w * dpr || canvas.height !== h * dpr) {
    canvas.width = w * dpr;
    canvas.height = h * dpr;
  }
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);

  const rects = tiles.map((t) => ({
    l: t.x * view.k + view.x,
    t: t.y * view.k + view.y,
    r: (t.x + t.width) * view.k + view.x,
    b: (t.y + t.height) * view.k + view.y
  }));
  const insideTile = (px: number, py: number): boolean => {
    for (const r of rects) {
      if (px >= r.l && px <= r.r && py >= r.t && py <= r.b) return true;
    }
    return false;
  };

  const step = CELL * view.k;
  const majorStep = MAJOR * view.k;
  const offX = ((view.x % majorStep) + majorStep) % majorStep;
  const offY = ((view.y % majorStep) + majorStep) % majorStep;
  const dotOffX = ((view.x % step) + step) % step;
  const dotOffY = ((view.y % step) + step) % step;
  const dotSize = Math.max(1, 1.5 * view.k);

  const minorFade = Math.min(1, Math.max(0, (view.k - 0.5) / (0.75 - 0.5)));
  if (minorFade > 0) {
    ctx.fillStyle = `rgba(255,255,255,${0.15 * minorFade})`;
    const halfDot = dotSize / 2;
    for (let x = dotOffX; x <= w; x += step) {
      for (let y = dotOffY; y <= h; y += step) {
        if (insideTile(x, y)) continue;
        ctx.fillRect(Math.round(x - halfDot), Math.round(y - halfDot), dotSize, dotSize);
      }
    }
  }

  const majorFade = Math.min(1, Math.max(0, (view.k - ZOOM_MIN) / (0.5 - ZOOM_MIN)));
  if (majorFade > 0) {
    const majorDotSize = Math.max(1.5, 1.5 * view.k);
    const halfMajor = majorDotSize / 2;
    ctx.fillStyle = `rgba(255,255,255,${0.25 * majorFade})`;
    for (let x = offX; x <= w; x += majorStep) {
      for (let y = offY; y <= h; y += majorStep) {
        if (insideTile(x, y)) continue;
        ctx.fillRect(Math.round(x - halfMajor), Math.round(y - halfMajor), majorDotSize, majorDotSize);
      }
    }
  }
};
