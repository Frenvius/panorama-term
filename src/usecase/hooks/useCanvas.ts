import React from 'react';

import type { Tile, View } from '~/domain/interfaces/canvas.interface';
import type { CanvasState } from '~/domain/interfaces/workspace.interface';
import { drawGrid } from '~/usecase/util/gridUtils';
import { restTarget } from '~/usecase/util/zoomUtils';
import { killPtySession } from '~/adapter/pty/sidecar.client';
import { toStored, toRuntime, type RuntimeCanvas } from '~/usecase/util/workspaceCanvas';
import {
  RUBBER_K,
  ZOOM_MIN,
  ZOOM_MAX,
  SNAP_DELAY,
  TILE_WIDTH,
  TILE_HEIGHT,
  INDICATOR_MS,
  TOOLBAR_HEIGHT,
  MAX_ZOOM_DELTA
} from '~/usecase/util/constants';

type PanOrigin = { ox: number; oy: number; vx: number; vy: number };

const createId = (): string => Math.random().toString(36).slice(2, 10);

const EMPTY: RuntimeCanvas = { tiles: [], frames: [], view: { x: 0, y: 0, k: 1 } };

interface UseCanvasArgs {
  seed: CanvasState | null;
  onPersist: (state: CanvasState) => void;
}

export const useCanvas = ({ seed, onPersist }: UseCanvasArgs) => {
  const initial = React.useRef(seed ? toRuntime(seed) : EMPTY);
  const frames = React.useRef(initial.current.frames);
  const [tiles, setTiles] = React.useState<Tile[]>(initial.current.tiles);
  const [view, setView] = React.useState<View>(initial.current.view);

  const bgRef = React.useRef<HTMLDivElement>(null);
  const gridRef = React.useRef<HTMLCanvasElement>(null);
  const indicatorRef = React.useRef<HTMLDivElement>(null);
  const indicatorTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const firstRender = React.useRef(true);

  const viewRef = React.useRef(view);
  viewRef.current = view;
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;

  const snapRaf = React.useRef(0);
  const focal = React.useRef({ x: 0, y: 0 });
  const snapTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const panRef = React.useRef<PanOrigin | null>(null);

  React.useEffect(() => {
    onPersist(toStored({ tiles, view, frames: frames.current }));
  }, [tiles, view, onPersist]);

  React.useEffect(() => {
    if (gridRef.current) drawGrid(gridRef.current, view, tiles);
  }, [view, tiles]);

  React.useEffect(() => {
    if (firstRender.current) {
      firstRender.current = false;
      return;
    }
    const el = indicatorRef.current;
    if (!el) return;
    el.textContent = `${Math.round(view.k * 100)}%`;
    el.setAttribute('data-visible', 'true');
    clearTimeout(indicatorTimer.current);
    indicatorTimer.current = setTimeout(() => el.removeAttribute('data-visible'), INDICATOR_MS);
  }, [view.k]);

  React.useEffect(() => {
    const bg = bgRef.current;
    if (!bg) return;
    let prevW = bg.clientWidth;
    let prevH = bg.clientHeight;
    const ro = new ResizeObserver(() => {
      const w = bg.clientWidth;
      const h = bg.clientHeight;
      setView((v) => ({ ...v, x: v.x + (w - prevW) / 2, y: v.y + (h - prevH) / 2 }));
      prevW = w;
      prevH = h;
      if (gridRef.current) drawGrid(gridRef.current, viewRef.current, tilesRef.current);
    });
    ro.observe(bg);
    return () => ro.disconnect();
  }, []);

  React.useEffect(
    () => () => {
      cancelAnimationFrame(snapRaf.current);
      clearTimeout(snapTimer.current);
      clearTimeout(indicatorTimer.current);
    },
    []
  );

  const addTile = React.useCallback(() => {
    setView((v) => {
      const cx = (window.innerWidth / 2 - v.x) / v.k;
      const cy = ((window.innerHeight - TOOLBAR_HEIGHT) / 2 - v.y) / v.k;
      setTiles((prev) => [
        ...prev,
        {
          id: createId(),
          type: 'term',
          x: cx - TILE_WIDTH / 2,
          y: cy - TILE_HEIGHT / 2,
          width: TILE_WIDTH,
          height: TILE_HEIGHT,
          zIndex: prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1
        }
      ]);
      return v;
    });
  }, []);

  const closeTile = React.useCallback((id: string) => {
    setTiles((prev) => prev.filter((t) => t.id !== id));
    void killPtySession(id);
  }, []);

  const moveTile = React.useCallback(
    (id: string, dx: number, dy: number) => {
      setTiles((prev) => prev.map((t) => (t.id === id ? { ...t, x: t.x + dx / view.k, y: t.y + dy / view.k } : t)));
    },
    [view.k]
  );

  const resetZoom = React.useCallback(() => {
    setView((v) => {
      const cx = window.innerWidth / 2;
      const cy = (window.innerHeight - TOOLBAR_HEIGHT) / 2;
      const wx = (cx - v.x) / v.k;
      const wy = (cy - v.y) / v.k;
      return { k: 1, x: cx - wx, y: cy - wy };
    });
  }, []);

  const snapBack = React.useCallback(() => {
    const animate = () => {
      let done = false;
      setView((v) => {
        const target = restTarget(v.k);
        let k = v.k + (target - v.k) * 0.15;
        if (Math.abs(k - target) < 0.001) {
          k = target;
          done = true;
        }
        const ratio = k / v.k - 1;
        return { k, x: v.x - (focal.current.x - v.x) * ratio, y: v.y - (focal.current.y - v.y) * ratio };
      });
      snapRaf.current = done ? 0 : requestAnimationFrame(animate);
    };
    snapRaf.current = requestAnimationFrame(animate);
  }, []);

  const onWheel = (e: React.WheelEvent) => {
    if ((e.target as Element).closest('[data-tile]')) return;
    if (e.shiftKey) {
      setView((v) => ({ ...v, x: v.x - (e.deltaX || e.deltaY) * 1.2 }));
      return;
    }
    if (e.ctrlKey) {
      setView((v) => ({ ...v, x: v.x - e.deltaX * 1.2, y: v.y - e.deltaY * 1.2 }));
      return;
    }
    cancelAnimationFrame(snapRaf.current);
    snapRaf.current = 0;
    clearTimeout(snapTimer.current);

    const rect = e.currentTarget.getBoundingClientRect();
    const px = e.clientX - rect.left;
    const py = e.clientY - rect.top;
    focal.current = { x: px, y: py };

    setView((v) => {
      const clamped = Math.sign(e.deltaY) * Math.min(Math.abs(e.deltaY), MAX_ZOOM_DELTA);
      let factor = Math.exp((-clamped * 0.6) / 100);
      if (v.k >= ZOOM_MAX && factor > 1) {
        const overshoot = v.k / ZOOM_MAX - 1;
        factor = 1 + (factor - 1) / (1 + overshoot * RUBBER_K);
      } else if (v.k <= ZOOM_MIN && factor < 1) {
        const overshoot = ZOOM_MIN / v.k - 1;
        factor = 1 - (1 - factor) / (1 + overshoot * RUBBER_K);
      }
      const k = v.k * factor;
      const ratio = k / v.k - 1;
      if (k > 1 || k < ZOOM_MIN) snapTimer.current = setTimeout(snapBack, SNAP_DELAY);
      return { k, x: v.x - (px - v.x) * ratio, y: v.y - (py - v.y) * ratio };
    });
  };

  const onBgPointerDown = (e: React.PointerEvent) => {
    if (e.target !== e.currentTarget && e.target !== gridRef.current) return;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    panRef.current = { ox: e.clientX, oy: e.clientY, vx: view.x, vy: view.y };
  };

  const onBgPointerMove = (e: React.PointerEvent) => {
    const p = panRef.current;
    if (!p) return;
    setView((v) => ({ ...v, x: p.vx + (e.clientX - p.ox), y: p.vy + (e.clientY - p.oy) }));
  };

  const endPan = () => {
    panRef.current = null;
  };

  return {
    view,
    tiles,
    bgRef,
    endPan,
    gridRef,
    onWheel,
    addTile,
    moveTile,
    closeTile,
    resetZoom,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  };
};
