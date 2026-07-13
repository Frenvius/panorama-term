import React from 'react';

import type { Tile, View, Frame, FrameMember } from '~/domain/interfaces/canvas.interface';
import type { CanvasState } from '~/domain/interfaces/workspace.interface';
import { drawGrid } from '~/usecase/util/gridUtils';
import { THEME_EVENT } from '~/usecase/util/theme';
import { getSetting } from '~/adapter/settings/settings.client';
import { restTarget } from '~/usecase/util/zoomUtils';
import { killPtySession } from '~/adapter/pty/sidecar.client';
import { computeDragSnap, computeResizeSnap } from '~/usecase/util/magneticSnap';
import { NOTE_DEFAULT_COLOR } from '~/usecase/util/note';
import { toStored, toRuntime, type RuntimeCanvas } from '~/usecase/util/workspaceCanvas';
import {
  CELL,
  SNAP_PX,
  RUBBER_K,
  ZOOM_MIN,
  ZOOM_MAX,
  SNAP_DELAY,
  TILE_WIDTH,
  NOTE_WIDTH,
  FRAME_COLOR,
  FRAME_WIDTH,
  NOTE_HEIGHT,
  MAX_ZOOM_KEY,
  TILE_HEIGHT,
  FRAME_HEIGHT,
  INDICATOR_MS,
  TILE_MIN_WIDTH,
  TOOLBAR_HEIGHT,
  MAX_ZOOM_DELTA,
  TILE_MIN_HEIGHT,
  FRAME_MIN_WIDTH,
  FRAME_MIN_HEIGHT
} from '~/usecase/util/constants';

const maxZoom = (): number => Math.min(ZOOM_MAX, Math.max(1, getSetting(MAX_ZOOM_KEY, 1)));

type PanOrigin = { ox: number; oy: number; vx: number; vy: number; moved: boolean; pan: boolean; activateId: string | null };

const createId = (): string => Math.random().toString(36).slice(2, 10);

const FOCUS_MS = 350;

const EMPTY: RuntimeCanvas = { tiles: [], frames: [], view: { x: 0, y: 0, k: 1 } };

interface UseCanvasArgs {
  seed: CanvasState | null;
  onPersist: (state: CanvasState) => void;
}

export const useCanvas = ({ seed, onPersist }: UseCanvasArgs) => {
  const initial = React.useRef(seed ? toRuntime(seed) : EMPTY);
  const [frames, setFrames] = React.useState<Frame[]>(initial.current.frames);
  const [tiles, setTiles] = React.useState<Tile[]>(initial.current.tiles);
  const [view, setView] = React.useState<View>(initial.current.view);
  const [activeTile, setActiveTile] = React.useState<string | null>(null);

  const bgRef = React.useRef<HTMLDivElement>(null);
  const gridRef = React.useRef<HTMLCanvasElement>(null);
  const indicatorRef = React.useRef<HTMLDivElement>(null);
  const indicatorTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const firstRender = React.useRef(true);

  const viewRef = React.useRef(view);
  viewRef.current = view;
  const tilesRef = React.useRef(tiles);
  tilesRef.current = tiles;
  const framesRef = React.useRef(frames);
  framesRef.current = frames;

  const snapRaf = React.useRef(0);
  const focusRaf = React.useRef(0);
  const focal = React.useRef({ x: 0, y: 0 });
  const snapTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const panRef = React.useRef<PanOrigin | null>(null);
  const resizeRaw = React.useRef<{ id: string; x: number; y: number; width: number; height: number } | null>(null);

  React.useEffect(() => {
    const id = setTimeout(() => onPersist(toStored({ tiles, view, frames })), 400);
    return () => clearTimeout(id);
  }, [tiles, view, frames, onPersist]);

  React.useEffect(() => {
    if (gridRef.current) drawGrid(gridRef.current, view);
  }, [view]);

  React.useEffect(() => {
    const onTheme = () => {
      if (gridRef.current) drawGrid(gridRef.current, viewRef.current);
    };
    window.addEventListener(THEME_EVENT, onTheme);
    return () => window.removeEventListener(THEME_EVENT, onTheme);
  }, []);

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
      if (!w || !h) return;
      if (focusRaf.current) {
        prevW = w;
        prevH = h;
        return;
      }
      const v = viewRef.current;
      const next = { ...v, x: v.x + (w - prevW) / 2, y: v.y + (h - prevH) / 2 };
      viewRef.current = next;
      setView(next);
      prevW = w;
      prevH = h;
      if (gridRef.current) drawGrid(gridRef.current, next);
    });
    ro.observe(bg);
    return () => ro.disconnect();
  }, []);

  React.useEffect(() => {
    const bg = bgRef.current;
    if (!bg) return;
    const onScroll = () => {
      bg.scrollLeft = 0;
      bg.scrollTop = 0;
    };
    bg.addEventListener('scroll', onScroll);
    return () => bg.removeEventListener('scroll', onScroll);
  }, []);

  React.useEffect(
    () => () => {
      cancelAnimationFrame(snapRaf.current);
      cancelAnimationFrame(focusRaf.current);
      clearTimeout(snapTimer.current);
      clearTimeout(indicatorTimer.current);
    },
    []
  );

  const addTile = React.useCallback((center?: { x: number; y: number }) => {
    setView((v) => {
      const cx = center ? center.x : (window.innerWidth / 2 - v.x) / v.k;
      const cy = center ? center.y : ((window.innerHeight - TOOLBAR_HEIGHT) / 2 - v.y) / v.k;
      setTiles((prev) => {
        const last = [...prev].reverse().find((t) => t.type === 'term');
        const width = last?.width ?? TILE_WIDTH;
        const height = last?.height ?? TILE_HEIGHT;
        return [
          ...prev,
          {
            id: createId(),
            type: 'term',
            x: cx - width / 2,
            y: cy - height / 2,
            width,
            height,
            zIndex: prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1
          }
        ];
      });
      return v;
    });
  }, []);

  const addNote = React.useCallback((center?: { x: number; y: number }) => {
    setView((v) => {
      const cx = center ? center.x : (window.innerWidth / 2 - v.x) / v.k;
      const cy = center ? center.y : ((window.innerHeight - TOOLBAR_HEIGHT) / 2 - v.y) / v.k;
      setTiles((prev) => [
        ...prev,
        {
          id: createId(),
          type: 'note',
          x: cx - NOTE_WIDTH / 2,
          y: cy - NOTE_HEIGHT / 2,
          width: NOTE_WIDTH,
          height: NOTE_HEIGHT,
          color: NOTE_DEFAULT_COLOR,
          content: '',
          zIndex: prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1
        }
      ]);
      return v;
    });
  }, []);

  const addCode = React.useCallback((cwd: string, filePath: string) => {
    setView((v) => {
      const cx = (window.innerWidth / 2 - v.x) / v.k;
      const cy = ((window.innerHeight - TOOLBAR_HEIGHT) / 2 - v.y) / v.k;
      setTiles((prev) => [
        ...prev,
        {
          id: createId(),
          type: 'code',
          cwd,
          filePath,
          autoTitle: filePath.split(/[\\/]/).pop(),
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

  const patchTile = React.useCallback((id: string, patch: Partial<Tile>) => {
    setTiles((prev) => prev.map((t) => (t.id === id ? { ...t, ...patch } : t)));
  }, []);

  const closeTile = React.useCallback((id: string) => {
    setTiles((prev) => prev.filter((t) => t.id !== id));
    setActiveTile((a) => (a === id ? null : a));
    void killPtySession(id);
  }, []);

  const duplicateTile = React.useCallback((id: string) => {
    const copyId = createId();
    setTiles((prev) => {
      const src = prev.find((t) => t.id === id);
      if (!src) return prev;
      const copy: Tile = {
        ...src,
        id: copyId,
        x: src.x + 24,
        y: src.y + 24,
        zIndex: prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1,
        ptySessionId: undefined,
        content: undefined
      };
      return [...prev, copy];
    });
    setActiveTile(copyId);
  }, []);

  const setTileCwd = React.useCallback((id: string, cwd: string, branch?: string) => {
    const autoTitle = cwd;
    setTiles((prev) =>
      prev.map((t) =>
        t.id === id && (t.cwd !== cwd || t.autoTitle !== autoTitle || t.branch !== branch)
          ? { ...t, cwd, autoTitle, branch }
          : t
      )
    );
  }, []);

  const setTileOscTitle = React.useCallback((id: string, title: string) => {
    const oscTitle = title.trim() || undefined;
    setTiles((prev) =>
      prev.map((t) => (t.id === id && t.oscTitle !== oscTitle ? { ...t, oscTitle } : t))
    );
  }, []);

  const moveTile = React.useCallback((id: string, rawX: number, rawY: number) => {
    const prev = tilesRef.current;
    const moving = prev.find((t) => t.id === id);
    if (!moving) return;
    const cand = { ...moving, x: rawX, y: rawY };
    const others = [...prev.filter((t) => t.id !== id), ...framesRef.current];
    const snap = computeDragSnap(cand, others, SNAP_PX / viewRef.current.k);
    setTiles((p) => p.map((t) => (t.id === id ? { ...t, x: snap.x ?? rawX, y: snap.y ?? rawY } : t)));
  }, []);

  const resizeTile = React.useCallback((id: string, dir: string, dx: number, dy: number) => {
    const k = viewRef.current.k;
    const wdx = dx / k;
    const wdy = dy / k;
    const prev = tilesRef.current;
    const tile = prev.find((t) => t.id === id);
    if (!tile) return;
    const raw = resizeRaw.current?.id === id ? resizeRaw.current : { id, x: tile.x, y: tile.y, width: tile.width, height: tile.height };
    let { x, y, width, height } = raw;
    if (dir.includes('e')) width = Math.max(TILE_MIN_WIDTH, width + wdx);
    if (dir.includes('s')) height = Math.max(TILE_MIN_HEIGHT, height + wdy);
    if (dir.includes('w')) {
      const nw = Math.max(TILE_MIN_WIDTH, width - wdx);
      x += width - nw;
      width = nw;
    }
    if (dir.includes('n')) {
      const nh = Math.max(TILE_MIN_HEIGHT, height - wdy);
      y += height - nh;
      height = nh;
    }
    resizeRaw.current = { id, x, y, width, height };
    const others = [...prev.filter((t) => t.id !== id), ...framesRef.current];
    const snap = computeResizeSnap({ x, y, width, height }, dir, others, SNAP_PX / k, TILE_MIN_WIDTH, TILE_MIN_HEIGHT);
    setTiles((p) => p.map((t) => (t.id === id ? { ...t, x: snap.x, y: snap.y, width: snap.width, height: snap.height } : t)));
  }, []);

  const snapTile = React.useCallback((id: string) => {
    resizeRaw.current = null;
    setTiles((prev) =>
      prev.map((t) => {
        if (t.id !== id) return t;
        const x = Math.round(t.x / CELL) * CELL;
        const y = Math.round(t.y / CELL) * CELL;
        const width = Math.max(TILE_MIN_WIDTH, Math.round(t.width / CELL) * CELL);
        const height = Math.max(TILE_MIN_HEIGHT, Math.round(t.height / CELL) * CELL);
        return { ...t, x, y, width, height };
      })
    );
  }, []);

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
      const v = viewRef.current;
      const target = restTarget(v.k, maxZoom());
      let k = v.k + (target - v.k) * 0.15;
      const done = Math.abs(k - target) < 0.001;
      if (done) k = target;
      const ratio = k / v.k - 1;
      const next = { k, x: v.x - (focal.current.x - v.x) * ratio, y: v.y - (focal.current.y - v.y) * ratio };
      viewRef.current = next;
      setView(next);
      snapRaf.current = done ? 0 : requestAnimationFrame(animate);
    };
    snapRaf.current = requestAnimationFrame(animate);
  }, []);

  const onWheel = (e: React.WheelEvent) => {
    const tileEl = (e.target as Element).closest('[data-tile]');
    if (tileEl && tileEl.getAttribute('data-tile') === activeTile) return;
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

    const ceil = maxZoom();
    const v = viewRef.current;
    const clamped = Math.sign(e.deltaY) * Math.min(Math.abs(e.deltaY), MAX_ZOOM_DELTA);
    let factor = Math.exp((-clamped * 0.6) / 100);
    if (v.k >= ceil && factor > 1) {
      const overshoot = v.k / ceil - 1;
      factor = 1 + (factor - 1) / (1 + overshoot * RUBBER_K);
    } else if (v.k <= ZOOM_MIN && factor < 1) {
      const overshoot = ZOOM_MIN / v.k - 1;
      factor = 1 - (1 - factor) / (1 + overshoot * RUBBER_K);
    }
    const k = v.k * factor;
    const ratio = k / v.k - 1;
    const next = { k, x: v.x - (px - v.x) * ratio, y: v.y - (py - v.y) * ratio };
    viewRef.current = next;
    setView(next);
    if (k > ceil || k < ZOOM_MIN) snapTimer.current = setTimeout(snapBack, SNAP_DELAY);
  };

  const onBgPointerDown = (e: React.PointerEvent) => {
    const tileEl = (e.target as Element).closest('[data-tile]');
    const tileId = tileEl?.getAttribute('data-tile') ?? null;
    const pan = e.button === 1;
    if (!pan && tileId && tileId === activeTile) return;
    if (pan) e.preventDefault();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    panRef.current = { ox: e.clientX, oy: e.clientY, vx: view.x, vy: view.y, moved: false, pan, activateId: tileId };
  };

  const onBgPointerMove = (e: React.PointerEvent) => {
    const p = panRef.current;
    if (!p) return;
    if (!p.moved && Math.hypot(e.clientX - p.ox, e.clientY - p.oy) > 4) p.moved = true;
    setView((v) => ({ ...v, x: p.vx + (e.clientX - p.ox), y: p.vy + (e.clientY - p.oy) }));
  };

  const endPan = () => {
    const p = panRef.current;
    panRef.current = null;
    if (p && !p.moved && !p.pan) setActiveTile(p.activateId);
  };

  const activateTile = React.useCallback((id: string) => setActiveTile(id), []);

  const addFrame = React.useCallback((x: number, y: number) => {
    setFrames((prev) => [
      ...prev,
      {
        id: createId(),
        x: Math.round(x / CELL) * CELL,
        y: Math.round(y / CELL) * CELL,
        width: FRAME_WIDTH,
        height: FRAME_HEIGHT,
        title: 'Frame',
        color: FRAME_COLOR
      }
    ]);
  }, []);

  const dragFrame = React.useCallback((id: string, x: number, y: number, members: FrameMember[]) => {
    const frame = framesRef.current.find((f) => f.id === id);
    if (!frame) return;
    const memberIds = new Set(members.map((m) => m.id));
    const others = [...tilesRef.current.filter((t) => !memberIds.has(t.id)), ...framesRef.current.filter((f) => f.id !== id)];
    const snap = computeDragSnap({ ...frame, x, y }, others, SNAP_PX / viewRef.current.k);
    const fx = snap.x ?? x;
    const fy = snap.y ?? y;
    const ddx = fx - x;
    const ddy = fy - y;
    setFrames((prev) => prev.map((f) => (f.id === id ? { ...f, x: fx, y: fy } : f)));
    if (members.length === 0) return;
    const pos = new Map(members.map((m) => [m.id, m]));
    setTiles((prev) => prev.map((t) => (pos.has(t.id) ? { ...t, x: pos.get(t.id)!.x + ddx, y: pos.get(t.id)!.y + ddy } : t)));
  }, []);

  const resizeFrame = React.useCallback((id: string, dir: string, dx: number, dy: number) => {
    const k = viewRef.current.k;
    const wdx = dx / k;
    const wdy = dy / k;
    const frame = framesRef.current.find((f) => f.id === id);
    if (!frame) return;
    const raw = resizeRaw.current?.id === id ? resizeRaw.current : { id, x: frame.x, y: frame.y, width: frame.width, height: frame.height };
    let { x, y, width, height } = raw;
    if (dir.includes('e')) width = Math.max(FRAME_MIN_WIDTH, width + wdx);
    if (dir.includes('s')) height = Math.max(FRAME_MIN_HEIGHT, height + wdy);
    if (dir.includes('w')) {
      const nw = Math.max(FRAME_MIN_WIDTH, width - wdx);
      x += width - nw;
      width = nw;
    }
    if (dir.includes('n')) {
      const nh = Math.max(FRAME_MIN_HEIGHT, height - wdy);
      y += height - nh;
      height = nh;
    }
    resizeRaw.current = { id, x, y, width, height };
    const others = [...tilesRef.current, ...framesRef.current.filter((f) => f.id !== id)];
    const snap = computeResizeSnap({ x, y, width, height }, dir, others, SNAP_PX / k, FRAME_MIN_WIDTH, FRAME_MIN_HEIGHT);
    setFrames((prev) => prev.map((f) => (f.id === id ? { ...f, x: snap.x, y: snap.y, width: snap.width, height: snap.height } : f)));
  }, []);

  const snapFrame = React.useCallback((id: string) => {
    resizeRaw.current = null;
    setFrames((prev) =>
      prev.map((f) => {
        if (f.id !== id) return f;
        const x = Math.round(f.x / CELL) * CELL;
        const y = Math.round(f.y / CELL) * CELL;
        const width = Math.max(FRAME_MIN_WIDTH, Math.round(f.width / CELL) * CELL);
        const height = Math.max(FRAME_MIN_HEIGHT, Math.round(f.height / CELL) * CELL);
        return { ...f, x, y, width, height };
      })
    );
  }, []);

  const renameFrame = React.useCallback((id: string, title: string) => {
    setFrames((prev) => prev.map((f) => (f.id === id ? { ...f, title } : f)));
  }, []);

  const recolorFrame = React.useCallback((id: string, color: string) => {
    setFrames((prev) => prev.map((f) => (f.id === id ? { ...f, color } : f)));
  }, []);

  const removeFrame = React.useCallback((id: string) => {
    setFrames((prev) => prev.filter((f) => f.id !== id));
  }, []);

  const panTo = React.useCallback((x: number, y: number) => setView((v) => ({ ...v, x, y })), []);

  const glide = React.useCallback((cx: number, cy: number, tk: number) => {
    const bg = bgRef.current;
    if (!bg) return;
    cancelAnimationFrame(focusRaf.current);
    cancelAnimationFrame(snapRaf.current);
    clearTimeout(snapTimer.current);
    const start = viewRef.current;
    const sx = (bg.clientWidth / 2 - start.x) / start.k;
    const sy = (bg.clientHeight / 2 - start.y) / start.k;
    const t0 = performance.now();
    const step = (now: number) => {
      const p = Math.min((now - t0) / FOCUS_MS, 1);
      const e = 1 - Math.pow(1 - p, 3);
      const k = start.k + (tk - start.k) * e;
      const wx = sx + (cx - sx) * e;
      const wy = sy + (cy - sy) * e;
      const next = { k, x: bg.clientWidth / 2 - wx * k, y: bg.clientHeight / 2 - wy * k };
      viewRef.current = next;
      setView(next);
      focusRaf.current = p < 1 ? requestAnimationFrame(step) : 0;
    };
    focusRaf.current = requestAnimationFrame(step);
  }, []);

  const pending = (): boolean => {
    const bg = bgRef.current;
    return document.hidden || !bg?.clientWidth || !bg.clientHeight;
  };

  const focusTile = React.useCallback(
    function run(id: string, zoomToMax = false) {
      const tile = tilesRef.current.find((t) => t.id === id);
      if (!tile) return;
      if (pending()) {
        focusRaf.current = requestAnimationFrame(() => run(id, zoomToMax));
        return;
      }
      const tk = zoomToMax ? maxZoom() : viewRef.current.k;
      glide(tile.x + tile.width / 2, tile.y + tile.height / 2, tk);
    },
    [glide]
  );

  const focusFrame = React.useCallback(
    function run(id: string) {
      const bg = bgRef.current;
      const frame = framesRef.current.find((f) => f.id === id);
      if (!frame) return;
      if (!bg || pending()) {
        focusRaf.current = requestAnimationFrame(() => run(id));
        return;
      }
      const fit = Math.min(bg.clientWidth / frame.width, bg.clientHeight / frame.height) * 0.9;
      glide(frame.x + frame.width / 2, frame.y + frame.height / 2, Math.min(Math.max(fit, ZOOM_MIN), maxZoom()));
    },
    [glide]
  );

  return {
    view,
    tiles,
    panTo,
    bgRef,
    frames,
    endPan,
    addNote,
    focusTile,
    focusFrame,
    gridRef,
    onWheel,
    addTile,
    addCode,
    patchTile,
    addFrame,
    duplicateTile,
    moveTile,
    snapTile,
    dragFrame,
    closeTile,
    snapFrame,
    activeTile,
    setTileCwd,
    setTileOscTitle,
    resetZoom,
    resizeTile,
    removeFrame,
    renameFrame,
    resizeFrame,
    recolorFrame,
    activateTile,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  };
};
