import React from 'react';

import { reorderTargetIndex } from '~/usecase/util/reorderTarget';

const DRAG_THRESHOLD_PX = 4;
const DURATION = 180;
const EDGE_PX = 32;
const MAX_SPEED = 12;
const ITEM_SELECTOR = '[data-reorder-id]';

export interface ListReorder {
  ids: string[];
  from: number;
  to: number;
}

interface ListReorderOpts {
  enabled: boolean;
  handleSelector?: string;
  onReorder: (move: ListReorder) => void;
}

interface Press {
  list: HTMLElement;
  row: HTMLElement;
  rows: HTMLElement[];
  ids: string[];
  tops: number[];
  heights: number[];
  scroll: HTMLElement | null;
  scrollTop: number;
  pointerId: number;
  startX: number;
  startY: number;
  y: number;
  from: number;
  to: number;
  dragging: boolean;
  frame: number;
}

const findScrollParent = (el: HTMLElement): HTMLElement | null => {
  let node: HTMLElement | null = el;
  while (node) {
    const overflow = getComputedStyle(node).overflowY;
    if (overflow === 'auto' || overflow === 'scroll') return node;
    node = node.parentElement;
  }
  return null;
};

const clamp = (value: number, min: number, max: number) => Math.max(min, Math.min(max, value));

export const useListReorder = ({ enabled, handleSelector, onReorder }: ListReorderOpts) => {
  const press = React.useRef<Press | null>(null);
  const suppressClick = React.useRef(false);
  const timer = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const clear = (current: Press) => {
    cancelAnimationFrame(current.frame);
    current.rows.forEach((row) => {
      row.style.transform = '';
      delete row.dataset.dragged;
    });
    delete current.list.dataset.dragging;
    delete current.list.dataset.settling;
    if (current.list.hasPointerCapture(current.pointerId)) current.list.releasePointerCapture(current.pointerId);
    if (press.current === current) press.current = null;
  };

  React.useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
    if (press.current) clear(press.current);
  }, []);

  const paint = (current: Press) => {
    const { tops, heights, from, rows } = current;
    const last = rows.length - 1;
    const delta = current.y - current.startY + ((current.scroll?.scrollTop ?? 0) - current.scrollTop);
    const offset = clamp(delta, tops[0] - tops[from], tops[last] + heights[last] - tops[from] - heights[from]);
    const rects = tops.map((top, index) => ({ top, height: heights[index] }));
    const target = reorderTargetIndex({ centerY: tops[from] + delta + heights[from] / 2, rects, from });
    const gap = from < last ? tops[from + 1] - tops[from] - heights[from] : tops[from] - tops[from - 1] - heights[from - 1];
    const step = heights[from] + gap;
    current.to = target;
    rows.forEach((row, index) => {
      let shift = 0;
      if (index > from && index <= target) shift = -step;
      if (index < from && index >= target) shift = step;
      row.style.transform = index === from ? `translateY(${offset}px) scale(1.02)` : `translateY(${shift}px)`;
    });
  };

  const tick = (current: Press) => {
    if (press.current !== current || !current.dragging) return;
    if (current.scroll) {
      const rect = current.scroll.getBoundingClientRect();
      let speed = 0;
      if (current.y < rect.top + EDGE_PX) speed = -Math.min(MAX_SPEED, (rect.top + EDGE_PX - current.y) / 2);
      else if (current.y > rect.bottom - EDGE_PX) speed = Math.min(MAX_SPEED, (current.y - rect.bottom + EDGE_PX) / 2);
      current.scroll.scrollTop += speed;
    }
    paint(current);
    current.frame = requestAnimationFrame(() => tick(current));
  };

  const handlePointerDown = (e: React.PointerEvent<HTMLElement>) => {
    suppressClick.current = false;
    if (!enabled || press.current || e.button !== 0 || !(e.target instanceof Element) || e.target.closest('button, input')) return;
    const row = e.target.closest<HTMLElement>(ITEM_SELECTOR);
    if (!row || row.parentElement !== e.currentTarget) return;
    if (handleSelector && !row.contains(e.target.closest(handleSelector))) return;
    const zone = row.dataset.reorderZone;
    const rows = (Array.from(e.currentTarget.children) as HTMLElement[]).filter(
      (item) => item.matches(ITEM_SELECTOR) && item.dataset.reorderZone === zone
    );
    if (rows.length < 2) return;
    const rects = rows.map((item) => item.getBoundingClientRect());
    const scroll = findScrollParent(e.currentTarget);
    press.current = {
      list: e.currentTarget, row, rows, ids: rows.map((item) => item.dataset.reorderId ?? ''),
      tops: rects.map((rect) => rect.top), heights: rects.map((rect) => rect.height),
      scroll, scrollTop: scroll?.scrollTop ?? 0, pointerId: e.pointerId, startX: e.clientX, startY: e.clientY, y: e.clientY,
      from: rows.indexOf(row), to: rows.indexOf(row), dragging: false, frame: 0
    };
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLElement>) => {
    const current = press.current;
    if (!current || e.pointerId !== current.pointerId) return;
    if (!current.dragging) {
      if (Math.hypot(e.clientX - current.startX, e.clientY - current.startY) < DRAG_THRESHOLD_PX) return;
      current.dragging = true;
      current.list.setPointerCapture(e.pointerId);
      current.list.dataset.dragging = 'true';
      current.row.dataset.dragged = 'true';
      current.frame = requestAnimationFrame(() => tick(current));
    }
    current.y = e.clientY;
    paint(current);
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLElement>) => {
    const current = press.current;
    if (!current || e.pointerId !== current.pointerId) return;
    if (!current.dragging) { clear(current); return; }
    suppressClick.current = true;
    cancelAnimationFrame(current.frame);
    current.dragging = false;
    current.list.dataset.settling = 'true';
    const { tops, heights, from, to } = current;
    const offset = tops[to] - tops[from] + (to > from ? heights[to] - heights[from] : 0);
    current.row.style.transform = `translateY(${offset}px) scale(1)`;
    const finish = () => {
      timer.current = null;
      clear(current);
      if (to !== from) onReorder({ ids: current.ids, from, to });
    };
    if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) finish();
    else timer.current = setTimeout(finish, DURATION);
  };

  const handlePointerCancel = () => {
    if (press.current) clear(press.current);
  };

  React.useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || !press.current) return;
      if (press.current.dragging) suppressClick.current = true;
      if (timer.current) { clearTimeout(timer.current); timer.current = null; }
      clear(press.current);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, []);

  const handleClickCapture = (e: React.MouseEvent<HTMLElement>) => {
    if (!suppressClick.current) return;
    suppressClick.current = false;
    e.stopPropagation();
  };

  return {
    'data-reorderable': enabled || undefined,
    onPointerDown: handlePointerDown,
    onPointerMove: handlePointerMove,
    onPointerUp: handlePointerUp,
    onPointerCancel: handlePointerCancel,
    onClickCapture: handleClickCapture
  };
};
