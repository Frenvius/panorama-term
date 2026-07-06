import React from 'react';

import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import { themeInk, THEME_EVENT } from '~/usecase/util/theme';

import styles from './styles.module.scss';

interface MinimapProps {
  view: View;
  tiles: Tile[];
  viewportRef: React.RefObject<HTMLDivElement | null>;
  onPan: (x: number, y: number) => void;
}

interface Bounds {
  minX: number;
  minY: number;
  width: number;
  height: number;
}

const MINIMAP_W = 160;
const MINIMAP_H = 112;
const PADDING_RATIO = 0.1;
const MIN_TILE_W = 4;
const MIN_TILE_H = 3;
const MIN_EXTENT_FACTOR = 3;
const TILE_OPACITY = 0.6;
const VP_BORDER_OPACITY = 0.55;
const SCRIM_OPACITY = 0.35;
const IDLE_MS = 1500;
const PAN_DURATION = 150;

const TILE_COLORS: Record<Tile['type'], string> = {
  term: '#4144e3',
  note: '#0bbac6',
  code: '#c9f82c',
  image: '#e12b3f',
  browser: '#9750e3',
  graph: '#e28143'
};

const easeOut = (t: number): number => 1 - Math.pow(1 - t, 3);

const Minimap = ({ view, tiles, viewportRef, onPan }: MinimapProps) => {
  const wrapperRef = React.useRef<HTMLDivElement>(null);
  const canvasRef = React.useRef<HTMLCanvasElement>(null);

  const viewRef = React.useRef(view);
  viewRef.current = view;
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;
  const onPanRef = React.useRef(onPan);
  onPanRef.current = onPan;

  const render = React.useRef({ scale: 1, offX: 0, offY: 0, bounds: null as Bounds | null });
  const idleTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const rafId = React.useRef(0);
  const animRaf = React.useRef(0);
  const drag = React.useRef({ active: false, dx: 0, dy: 0 });

  const vpSize = React.useCallback(() => {
    const el = viewportRef.current;
    return { w: el?.clientWidth ?? 0, h: el?.clientHeight ?? 0 };
  }, [viewportRef]);

  const computeBounds = React.useCallback((): Bounds | null => {
    const items = tilesRef.current;
    if (items.length === 0) return null;

    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;
    for (const t of items) {
      if (t.x < minX) minX = t.x;
      if (t.y < minY) minY = t.y;
      if (t.x + t.width > maxX) maxX = t.x + t.width;
      if (t.y + t.height > maxY) maxY = t.y + t.height;
    }

    let bw = maxX - minX;
    let bh = maxY - minY;
    const { w: vw, h: vh } = vpSize();
    const minW = MIN_EXTENT_FACTOR * vw;
    const minH = MIN_EXTENT_FACTOR * vh;

    if (bw < minW) {
      const cx = (minX + maxX) / 2;
      minX = cx - minW / 2;
      maxX = cx + minW / 2;
      bw = minW;
    }
    if (bh < minH) {
      const cy = (minY + maxY) / 2;
      minY = cy - minH / 2;
      maxY = cy + minH / 2;
      bh = minH;
    }

    const padW = bw * PADDING_RATIO;
    const padH = bh * PADDING_RATIO;
    minX -= padW;
    minY -= padH;
    maxX += padW;
    maxY += padH;

    return { minX, minY, width: maxX - minX, height: maxY - minY };
  }, [vpSize]);

  const worldToMinimap = React.useCallback((wx: number, wy: number) => {
    const { scale, offX, offY, bounds } = render.current;
    if (!bounds) return { x: 0, y: 0 };
    return { x: (wx - bounds.minX) * scale + offX, y: (wy - bounds.minY) * scale + offY };
  }, []);

  const minimapToWorld = React.useCallback((mx: number, my: number) => {
    const { scale, offX, offY, bounds } = render.current;
    if (!bounds) return { x: 0, y: 0 };
    return { x: (mx - offX) / scale + bounds.minX, y: (my - offY) / scale + bounds.minY };
  }, []);

  const viewportRect = React.useCallback(() => {
    const { bounds } = render.current;
    if (!bounds) return null;
    const v = viewRef.current;
    const { w: vw, h: vh } = vpSize();
    const pos = worldToMinimap(-v.x / v.k, -v.y / v.k);
    return { x: pos.x, y: pos.y, w: (vw / v.k) * render.current.scale, h: (vh / v.k) * render.current.scale };
  }, [vpSize, worldToMinimap]);

  const draw = React.useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    ctx.clearRect(0, 0, MINIMAP_W, MINIMAP_H);
    const items = tilesRef.current;
    if (items.length === 0) return;

    const bounds = computeBounds();
    if (!bounds) return;
    const scale = Math.min(MINIMAP_W / bounds.width, MINIMAP_H / bounds.height);
    render.current = {
      scale,
      offX: (MINIMAP_W - bounds.width * scale) / 2,
      offY: (MINIMAP_H - bounds.height * scale) / 2,
      bounds
    };

    for (const tile of items) {
      const pos = worldToMinimap(tile.x, tile.y);
      const w = Math.max(tile.width * scale, MIN_TILE_W);
      const h = Math.max(tile.height * scale, MIN_TILE_H);
      ctx.globalAlpha = TILE_OPACITY;
      ctx.fillStyle = TILE_COLORS[tile.type] ?? '#888888';
      ctx.fillRect(pos.x + 0.5, pos.y + 0.5, w - 1, h - 1);
    }

    const rect = viewportRect();
    if (!rect) return;
    let { x: vx, y: vy, w: vw, h: vh } = rect;
    if (vx < 0) {
      vw += vx;
      vx = 0;
    }
    if (vy < 0) {
      vh += vy;
      vy = 0;
    }
    if (vx + vw > MINIMAP_W) vw = MINIMAP_W - vx;
    if (vy + vh > MINIMAP_H) vh = MINIMAP_H - vy;
    const visible = vw > 0 && vh > 0;

    ctx.globalAlpha = SCRIM_OPACITY;
    ctx.fillStyle = '#000000';
    ctx.beginPath();
    ctx.rect(0, 0, MINIMAP_W, MINIMAP_H);
    if (visible) ctx.rect(vx, vy, vw, vh);
    ctx.fill('evenodd');

    if (visible) {
      ctx.globalAlpha = VP_BORDER_OPACITY;
      ctx.strokeStyle = themeInk.line;
      ctx.lineWidth = 1;
      ctx.strokeRect(vx, vy, vw, vh);
    }
    ctx.globalAlpha = 1;
  }, [computeBounds, viewportRect, worldToMinimap]);

  const scheduleRedraw = React.useCallback(() => {
    if (rafId.current) return;
    rafId.current = requestAnimationFrame(() => {
      rafId.current = 0;
      draw();
    });
  }, [draw]);

  React.useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = MINIMAP_W * dpr;
    canvas.height = MINIMAP_H * dpr;
    const ctx = canvas.getContext('2d');
    ctx?.setTransform(dpr, 0, 0, dpr, 0, 0);
    scheduleRedraw();
  }, [scheduleRedraw]);

  React.useEffect(() => {
    const onTheme = () => scheduleRedraw();
    window.addEventListener(THEME_EVENT, onTheme);
    return () => window.removeEventListener(THEME_EVENT, onTheme);
  }, [scheduleRedraw]);

  React.useEffect(() => {
    const wrap = wrapperRef.current;
    if (!wrap) return;
    const show = tiles.length > 0;
    wrap.setAttribute('data-visible', show ? 'true' : 'false');
    if (show) {
      wrap.setAttribute('data-idle', 'false');
      clearTimeout(idleTimer.current);
      idleTimer.current = setTimeout(() => wrap.setAttribute('data-idle', 'true'), IDLE_MS);
    }
    scheduleRedraw();
  }, [view, tiles, scheduleRedraw]);

  React.useEffect(
    () => () => {
      clearTimeout(idleTimer.current);
      cancelAnimationFrame(rafId.current);
      cancelAnimationFrame(animRaf.current);
    },
    []
  );

  const localPoint = (e: React.PointerEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return {
      x: (e.clientX - rect.left) * (MINIMAP_W / rect.width),
      y: (e.clientY - rect.top) * (MINIMAP_H / rect.height)
    };
  };

  const insideViewport = (mx: number, my: number) => {
    const r = viewportRect();
    if (!r) return false;
    const tol = 4;
    return mx >= r.x - tol && mx <= r.x + r.w + tol && my >= r.y - tol && my <= r.y + r.h + tol;
  };

  const panToWorldCenter = (mx: number, my: number) => {
    const world = minimapToWorld(mx, my);
    const { w: vw, h: vh } = vpSize();
    const k = viewRef.current.k;
    onPanRef.current(vw / 2 - world.x * k, vh / 2 - world.y * k);
  };

  const animatePanTo = (targetX: number, targetY: number) => {
    cancelAnimationFrame(animRaf.current);
    const startX = viewRef.current.x;
    const startY = viewRef.current.y;
    const startTime = performance.now();
    const step = (now: number) => {
      const t = Math.min((now - startTime) / PAN_DURATION, 1);
      const e = easeOut(t);
      onPanRef.current(startX + (targetX - startX) * e, startY + (targetY - startY) * e);
      if (t < 1) animRaf.current = requestAnimationFrame(step);
    };
    animRaf.current = requestAnimationFrame(step);
  };

  const onPointerDown = (e: React.PointerEvent) => {
    e.stopPropagation();
    const pt = localPoint(e);
    if (insideViewport(pt.x, pt.y)) {
      const r = viewportRect()!;
      drag.current = { active: true, dx: pt.x - r.x, dy: pt.y - r.y };
      canvasRef.current!.setPointerCapture(e.pointerId);
      canvasRef.current!.style.cursor = 'grabbing';
    } else {
      const world = minimapToWorld(pt.x, pt.y);
      const { w: vw, h: vh } = vpSize();
      const k = viewRef.current.k;
      animatePanTo(vw / 2 - world.x * k, vh / 2 - world.y * k);
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    e.stopPropagation();
    const pt = localPoint(e);
    if (drag.current.active) {
      const r = viewportRect();
      if (!r) return;
      panToWorldCenter(pt.x - drag.current.dx + r.w / 2, pt.y - drag.current.dy + r.h / 2);
    } else {
      canvasRef.current!.style.cursor = insideViewport(pt.x, pt.y) ? 'grab' : 'crosshair';
    }
  };

  const onPointerUp = (e: React.PointerEvent) => {
    e.stopPropagation();
    if (!drag.current.active) return;
    drag.current.active = false;
    canvasRef.current!.releasePointerCapture(e.pointerId);
    const pt = localPoint(e);
    canvasRef.current!.style.cursor = insideViewport(pt.x, pt.y) ? 'grab' : 'crosshair';
  };

  return (
    <div ref={wrapperRef} className={styles.wrapper} data-visible="false">
      <canvas
        ref={canvasRef}
        className={styles.canvas}
        onPointerUp={onPointerUp}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
      />
    </div>
  );
};

export default Minimap;
