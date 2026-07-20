import React from 'react';
import { listen } from '@tauri-apps/api/event';

import type { Tile, View, Frame, FrameMember } from '~/domain/interfaces/canvas.interface';
import type { CanvasState } from '~/domain/interfaces/workspace.interface';
import { drawGrid } from '~/usecase/util/gridUtils';
import { THEME_EVENT } from '~/usecase/util/theme';
import { tileInFrame } from '~/usecase/util/frame';
import { getSetting } from '~/adapter/settings/settings.client';
import { restTarget } from '~/usecase/util/zoomUtils';
import { killPtySession } from '~/adapter/pty/sidecar.client';
import { linkNote, unlinkNote, deleteNote, linkTerm, unlinkTerm } from '~/adapter/notes/notes.client';
import { computeDragSnap, computeResizeSnap } from '~/usecase/util/magneticSnap';
import { termName, noteLinkTitle } from '~/usecase/util/noteLink';
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
  FRAME_PAD_KEY,
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

const CLOSED_KEY = 'panorama:closedTiles';
const CLOSED_MAX = 20;

const loadClosed = (ws: string | null): Tile[] => {
  if (!ws) return [];
  try {
    return JSON.parse(localStorage.getItem(`${CLOSED_KEY}:${ws}`) || '[]');
  } catch {
    return [];
  }
};

const saveClosed = (ws: string | null, stack: Tile[]): void => {
  if (!ws) return;
  try {
    localStorage.setItem(`${CLOSED_KEY}:${ws}`, JSON.stringify(stack.slice(-CLOSED_MAX)));
  } catch {}
};

const FOCUS_MS = 350;

const FRAME_PAD = 2 * CELL;

const frameBounds = (members: Tile[], pad: number): { x: number; y: number; width: number; height: number } => {
  const x = Math.min(...members.map((t) => t.x)) - pad;
  const y = Math.min(...members.map((t) => t.y)) - pad;
  const w = Math.max(...members.map((t) => t.x + t.width)) + pad - x;
  const h = Math.max(...members.map((t) => t.y + t.height)) + pad - y;
  return {
    x: Math.round(x / CELL) * CELL,
    y: Math.round(y / CELL) * CELL,
    width: Math.max(FRAME_MIN_WIDTH, Math.round(w / CELL) * CELL),
    height: Math.max(FRAME_MIN_HEIGHT, Math.round(h / CELL) * CELL)
  };
};

const EMPTY: RuntimeCanvas = { tiles: [], frames: [], view: { x: 0, y: 0, k: 1 } };

interface UseCanvasArgs {
  seed: CanvasState | null;
  wsId: string | null;
  onPersist: (state: CanvasState) => void;
}

export const useCanvas = ({ seed, wsId, onPersist }: UseCanvasArgs) => {
  const wsIdRef = React.useRef(wsId);
  wsIdRef.current = wsId;
  const initial = React.useRef(seed ? toRuntime(seed) : EMPTY);
  const [frames, setFrames] = React.useState<Frame[]>(initial.current.frames);
  const [tiles, setTiles] = React.useState<Tile[]>(initial.current.tiles);
  const [view, setView] = React.useState<View>(initial.current.view);
  const [activeTile, setActiveTile] = React.useState<string | null>(null);
  const [selected, setSelected] = React.useState<Set<string>>(() => new Set());
  const [marquee, setMarquee] = React.useState<{ x: number; y: number; width: number; height: number } | null>(null);

  const noteRenderDefault = React.useRef(false);
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
  const selectedRef = React.useRef(selected);
  selectedRef.current = selected;
  const marqueeRef = React.useRef<{ ox: number; oy: number; add: boolean } | null>(null);

  const snapRaf = React.useRef(0);
  const focusRaf = React.useRef(0);
  const focal = React.useRef({ x: 0, y: 0 });
  const snapTimer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const panRef = React.useRef<PanOrigin | null>(null);
  const resizeRaw = React.useRef<{ id: string; x: number; y: number; width: number; height: number } | null>(null);
  const lastTermSize = React.useRef<{ width: number; height: number } | null>(null);
  const mouseRef = React.useRef<{ x: number; y: number } | null>(null);

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

  React.useEffect(() => {
    const onMove = (e: PointerEvent) => {
      const rect = bgRef.current?.getBoundingClientRect();
      if (!rect) return;
      const v = viewRef.current;
      mouseRef.current = { x: (e.clientX - rect.left - v.x) / v.k, y: (e.clientY - rect.top - v.y) / v.k };
    };
    window.addEventListener('pointermove', onMove);
    return () => window.removeEventListener('pointermove', onMove);
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

  const addTile = React.useCallback((at?: { x: number; y: number }) => {
    setView((v) => {
      const p = at ?? mouseRef.current;
      const cx = p ? p.x : (window.innerWidth / 2 - v.x) / v.k;
      const cy = p ? p.y : ((window.innerHeight - TOOLBAR_HEIGHT) / 2 - v.y) / v.k;
      setTiles((prev) => {
        const last = lastTermSize.current ?? [...prev].reverse().find((t) => t.type === 'term');
        const width = last?.width ?? TILE_WIDTH;
        const height = last?.height ?? TILE_HEIGHT;
        return [
          ...prev,
          {
            id: createId(),
            type: 'term',
            x: cx,
            y: cy,
            width,
            height,
            zIndex: prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1
          }
        ];
      });
      return v;
    });
  }, []);

  const addRunView = React.useCallback((srcId: string, cwd: string, sessionId: string, cmd: string) => {
    setTiles((prev) => {
      const existing = prev.find((t) => t.runCwd && t.ptySessionId === sessionId);
      if (existing) {
        setActiveTile(existing.id);
        return prev;
      }
      const src = prev.find((t) => t.id === srcId);
      const width = src?.width ?? TILE_WIDTH;
      const height = src?.height ?? TILE_HEIGHT;
      const id = createId();
      setActiveTile(id);
      return [
        ...prev,
        {
          id,
          type: 'term' as const,
          runCwd: cwd,
          ptySessionId: sessionId,
          cwd,
          autoTitle: cmd,
          x: src ? src.x + src.width : 0,
          y: src ? src.y : 0,
          width,
          height,
          zIndex: prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1
        }
      ];
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
          renderOnly: noteRenderDefault.current,
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

  React.useEffect(() => {
    const off = listen<{ noteId: string; content: string }>('note:changed', (e) => {
      const { noteId, content } = e.payload;
      setTiles((prev) => prev.map((t) => (t.id === noteId && t.type === 'note' && t.content !== content ? { ...t, content } : t)));
    });
    return () => {
      void off.then((un) => un());
    };
  }, []);

  const bindKey = React.useCallback((id: string) => {
    const tile = tilesRef.current.find((t) => t.id === id);
    return tile?.ptySessionId ?? id;
  }, []);

  const linkNoteTo = React.useCallback((noteId: string, termId: string) => {
    const note = tilesRef.current.find((t) => t.id === noteId);
    if (!note || !wsIdRef.current) return;
    setTiles((prev) =>
      prev.map((t) => (t.id === noteId ? { ...t, linkedTo: [...new Set([...(t.linkedTo ?? []), termId])] } : t))
    );
    void linkNote(wsIdRef.current, noteId, bindKey(termId), noteLinkTitle(note)).catch(() => {});
  }, [bindKey]);

  const unlinkNoteFrom = React.useCallback((noteId: string, termId: string) => {
    setTiles((prev) =>
      prev.map((t) => (t.id === noteId ? { ...t, linkedTo: (t.linkedTo ?? []).filter((id) => id !== termId) } : t))
    );
    void unlinkNote(noteId, bindKey(termId)).catch(() => {});
  }, [bindKey]);

  const linkTermTo = React.useCallback((termId: string, peerId: string) => {
    const a = tilesRef.current.find((t) => t.id === termId);
    const b = tilesRef.current.find((t) => t.id === peerId);
    if (!a || !b || termId === peerId || (b.linkedTo ?? []).includes(termId)) return;
    setTiles((prev) =>
      prev.map((t) => (t.id === termId ? { ...t, linkedTo: [...new Set([...(t.linkedTo ?? []), peerId])] } : t))
    );
    void linkTerm(bindKey(termId), termName(a), bindKey(peerId), termName(b)).catch(() => {});
  }, [bindKey]);

  const unlinkTermFrom = React.useCallback((termId: string, peerId: string) => {
    setTiles((prev) =>
      prev.map((t) =>
        t.id === termId || t.id === peerId
          ? { ...t, linkedTo: (t.linkedTo ?? []).filter((id) => id !== termId && id !== peerId) }
          : t
      )
    );
    void unlinkTerm(bindKey(termId), bindKey(peerId)).catch(() => {});
  }, [bindKey]);

  const closeTile = React.useCallback((id: string) => {
    const closing = tilesRef.current.find((t) => t.id === id);
    if (closing && !closing.runCwd) {
      const ws = wsIdRef.current;
      saveClosed(ws, [...loadClosed(ws), closing]);
    }
    if (closing?.type === 'note') {
      for (const termId of closing.linkedTo ?? []) void unlinkNote(id, bindKey(termId)).catch(() => {});
      if (wsIdRef.current) void deleteNote(wsIdRef.current, id).catch(() => {});
    }
    if (closing?.type === 'term') {
      for (const peerId of closing.linkedTo ?? []) void unlinkTerm(bindKey(id), bindKey(peerId)).catch(() => {});
      for (const t of tilesRef.current) {
        if (!(t.linkedTo ?? []).includes(id)) continue;
        if (t.type === 'note') void unlinkNote(t.id, bindKey(id)).catch(() => {});
        if (t.type === 'term') void unlinkTerm(bindKey(t.id), bindKey(id)).catch(() => {});
      }
    }
    setTiles((prev) =>
      prev
        .filter((t) => t.id !== id)
        .map((t) => ((t.linkedTo ?? []).includes(id) ? { ...t, linkedTo: (t.linkedTo ?? []).filter((x) => x !== id) } : t))
    );
    setActiveTile((a) => (a === id ? null : a));
    void killPtySession(id);
  }, [bindKey]);

  const reopenTile = React.useCallback(() => {
    const ws = wsIdRef.current;
    const stack = loadClosed(ws);
    const tile = stack.pop();
    saveClosed(ws, stack);
    if (!tile) return;
    setTiles((prev) => {
      if (prev.some((t) => t.id === tile.id)) return prev;
      const zIndex = prev.reduce((m, t) => Math.max(m, t.zIndex), 0) + 1;
      return [...prev, { ...tile, zIndex, ptySessionId: undefined }];
    });
    setActiveTile(tile.id);
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
    const sel = selectedRef.current;
    if (sel.has(id) && sel.size > 1) {
      const dx = rawX - moving.x;
      const dy = rawY - moving.y;
      const members = prev.filter((t) => sel.has(t.id) && !t.pinned);
      if (!members.length) return;
      const minX = Math.min(...members.map((t) => t.x));
      const minY = Math.min(...members.map((t) => t.y));
      const maxX = Math.max(...members.map((t) => t.x + t.width));
      const maxY = Math.max(...members.map((t) => t.y + t.height));
      const box = { x: minX + dx, y: minY + dy, width: maxX - minX, height: maxY - minY };
      const rest = [...prev.filter((t) => !sel.has(t.id)), ...framesRef.current];
      const snap = computeDragSnap(box, rest, SNAP_PX / viewRef.current.k);
      const fdx = dx + (snap.x ?? box.x) - box.x;
      const fdy = dy + (snap.y ?? box.y) - box.y;
      setTiles((p) => p.map((t) => (sel.has(t.id) && !t.pinned ? { ...t, x: t.x + fdx, y: t.y + fdy } : t)));
      return;
    }
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
    if (tile.type === 'term') {
      lastTermSize.current = {
        width: Math.max(TILE_MIN_WIDTH, Math.round(snap.width / CELL) * CELL),
        height: Math.max(TILE_MIN_HEIGHT, Math.round(snap.height / CELL) * CELL)
      };
    }
    setTiles((p) => p.map((t) => (t.id === id ? { ...t, x: snap.x, y: snap.y, width: snap.width, height: snap.height } : t)));
  }, []);

  const snapTile = React.useCallback((id: string) => {
    resizeRaw.current = null;
    const sel = selectedRef.current;
    const group = sel.has(id) && sel.size > 1 ? sel : new Set([id]);
    setTiles((prev) =>
      prev.map((t) => {
        if (!group.has(t.id)) return t;
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
      let k = v.k + (target - v.k) * 0.4;
      const done = Math.abs(k - target) < 0.002;
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
    if (e.shiftKey) {
      setView((v) => ({ ...v, x: v.x - (e.deltaX || e.deltaY) * 1.2 }));
      return;
    }
    if (e.ctrlKey) {
      setView((v) => ({ ...v, x: v.x - e.deltaX * 1.2, y: v.y - e.deltaY * 1.2 }));
      return;
    }
    const tileEl = (e.target as Element).closest('[data-tile]');
    if (tileEl && tileEl.getAttribute('data-tile') === activeTile) return;
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
    cancelAnimationFrame(snapRaf.current);
    snapRaf.current = 0;
    clearTimeout(snapTimer.current);
    if (pan) e.preventDefault();
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    if (!pan && !tileId && e.button === 0) {
      marqueeRef.current = { ox: e.clientX, oy: e.clientY, add: e.shiftKey };
      return;
    }
    panRef.current = { ox: e.clientX, oy: e.clientY, vx: view.x, vy: view.y, moved: false, pan, activateId: tileId };
  };

  const onBgPointerMove = (e: React.PointerEvent) => {
    const m = marqueeRef.current;
    if (m) {
      const rect = bgRef.current?.getBoundingClientRect();
      if (!rect) return;
      setMarquee({
        x: Math.min(m.ox, e.clientX) - rect.left,
        y: Math.min(m.oy, e.clientY) - rect.top,
        width: Math.abs(e.clientX - m.ox),
        height: Math.abs(e.clientY - m.oy)
      });
      return;
    }
    const p = panRef.current;
    if (!p) return;
    if (!p.moved && Math.hypot(e.clientX - p.ox, e.clientY - p.oy) > 4) p.moved = true;
    if (p.pan) setView((v) => ({ ...v, x: p.vx + (e.clientX - p.ox), y: p.vy + (e.clientY - p.oy) }));
  };

  const endPan = () => {
    const m = marqueeRef.current;
    if (m) {
      marqueeRef.current = null;
      const box = marquee;
      setMarquee(null);
      if (!box || (box.width < 4 && box.height < 4)) {
        setActiveTile(null);
        if (!m.add) setSelected((prev) => (prev.size ? new Set() : prev));
        return;
      }
      const v = viewRef.current;
      const x1 = (box.x - v.x) / v.k;
      const y1 = (box.y - v.y) / v.k;
      const x2 = (box.x + box.width - v.x) / v.k;
      const y2 = (box.y + box.height - v.y) / v.k;
      const hits = tilesRef.current
        .filter((t) => t.x < x2 && t.x + t.width > x1 && t.y < y2 && t.y + t.height > y1)
        .map((t) => t.id);
      setSelected((prev) => (m.add ? new Set([...prev, ...hits]) : new Set(hits)));
      setActiveTile(null);
      return;
    }
    const p = panRef.current;
    panRef.current = null;
    const k = viewRef.current.k;
    if (k > maxZoom() || k < ZOOM_MIN) snapBack();
    if (p && !p.moved && !p.pan) {
      if (p.activateId && selectedRef.current.has(p.activateId)) return;
      setActiveTile(p.activateId);
      if (p.activateId) setSelected((prev) => (prev.size && !prev.has(p.activateId!) ? new Set() : prev));
    }
  };

  const activateTile = React.useCallback((id: string) => {
    setActiveTile(id);
    setSelected((prev) => (prev.size && !prev.has(id) ? new Set() : prev));
  }, []);

  const toggleSelect = React.useCallback((id: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const clearSelection = React.useCallback(() => setSelected((prev) => (prev.size ? new Set() : prev)), []);

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

  const removeFrameWithTiles = React.useCallback((id: string) => {
    const frame = framesRef.current.find((f) => f.id === id);
    if (!frame) return;
    const members = new Set(tilesRef.current.filter((t) => tileInFrame(frame, t)).map((t) => t.id));
    setFrames((prev) => prev.filter((f) => f.id !== id));
    setTiles((prev) => prev.filter((t) => !members.has(t.id)));
    setActiveTile((a) => (a && members.has(a) ? null : a));
    for (const tid of members) void killPtySession(tid);
  }, []);

  const fitFrame = React.useCallback((id: string) => {
    const frame = framesRef.current.find((f) => f.id === id);
    if (!frame) return;
    const members = tilesRef.current.filter((t) => tileInFrame(frame, t));
    if (!members.length) return;
    const box = frameBounds(members, getSetting(FRAME_PAD_KEY, 0));
    setFrames((prev) => prev.map((f) => (f.id === id ? { ...f, ...box } : f)));
  }, []);

  const frameSelection = React.useCallback(() => {
    const sel = selectedRef.current;
    const members = tilesRef.current.filter((t) => sel.has(t.id));
    if (!members.length) return;
    const box = frameBounds(members, FRAME_PAD);
    setFrames((prev) => [...prev, { id: createId(), ...box, title: 'Frame', color: FRAME_COLOR }]);
    setSelected((prev) => (prev.size ? new Set() : prev));
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
    marquee,
    selected,
    toggleSelect,
    clearSelection,
    addNote,
    noteRenderDefault,
    focusTile,
    focusFrame,
    gridRef,
    onWheel,
    addTile,
    addCode,
    addRunView,
    patchTile,
    addFrame,
    duplicateTile,
    moveTile,
    snapTile,
    linkNoteTo,
    unlinkNoteFrom,
    linkTermTo,
    unlinkTermFrom,
    dragFrame,
    closeTile,
    reopenTile,
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
    removeFrameWithTiles,
    fitFrame,
    frameSelection,
    activateTile,
    indicatorRef,
    onBgPointerMove,
    onBgPointerDown
  };
};
