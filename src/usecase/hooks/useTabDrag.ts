import type { DropZone } from '~/domain/interfaces/projectView.interface';
import type { DraggingTabState } from '~/components/ProjectView/types';

import React from 'react';

const DRAG_THRESHOLD_PX = 4;
const TAB_SELECTOR = '[data-tab-id]';
const TAB_BAR_SELECTOR = '[data-tab-bar-group]';
const BODY_SELECTOR = '[data-panel-body-group]';

export interface TabDropPayload {
  zone: DropZone;
  tileId: string;
  targetIndex?: number;
  targetGroupId: string;
}

export interface UseTabDragOpts {
  groupId: string;
  renamingTileId: string | null;
  draggingTab: DraggingTabState | null;
  onDropTab: (opts: TabDropPayload) => void;
  onDragStartTab: (drag: DraggingTabState | null) => void;
}

interface PressState {
  startX: number;
  startY: number;
  tileId: string;
  tab: HTMLElement;
  pointerId: number;
  dragging: boolean;
  cleanup: () => void;
}

export const computeDropZone = (rect: DOMRect, clientX: number, clientY: number): DropZone => {
  const rx = rect.width > 0 ? (clientX - rect.left) / rect.width : 0.5;
  const ry = rect.height > 0 ? (clientY - rect.top) / rect.height : 0.5;
  if (rx < 0.25) return 'left';
  if (rx > 0.75) return 'right';
  if (ry < 0.25) return 'top';
  if (ry > 0.75) return 'bottom';
  return 'center';
};

export const hitTestTabDrop = (clientX: number, clientY: number): Omit<TabDropPayload, 'tileId'> | null => {
  const hit = document.elementFromPoint(clientX, clientY);
  if (!hit) return null;

  const tabBar = hit.closest<HTMLElement>(TAB_BAR_SELECTOR);
  if (tabBar) {
    const targetGroupId = tabBar.dataset.tabBarGroup;
    if (!targetGroupId) return null;
    const tabs = Array.from(tabBar.querySelectorAll<HTMLElement>(TAB_SELECTOR));
    let targetIndex = tabs.length;
    for (let i = 0; i < tabs.length; i++) {
      const rect = tabs[i]!.getBoundingClientRect();
      if (clientX < rect.left + rect.width / 2) {
        targetIndex = i;
        break;
      }
    }
    return { targetGroupId, zone: 'center', targetIndex };
  }

  const body = hit.closest<HTMLElement>(BODY_SELECTOR);
  if (body) {
    const targetGroupId = body.dataset.panelBodyGroup;
    if (!targetGroupId) return null;
    const zone = computeDropZone(body.getBoundingClientRect(), clientX, clientY);
    return { targetGroupId, zone };
  }

  return null;
};

export const useTabDrag = ({ groupId, draggingTab, renamingTileId, onDropTab, onDragStartTab }: UseTabDragOpts) => {
  const pressRef = React.useRef<PressState | null>(null);
  const suppressClickRef = React.useRef(false);
  const latestRef = React.useRef({ groupId, onDropTab, onDragStartTab });
  latestRef.current = { groupId, onDropTab, onDragStartTab };

  const [dropZone, setDropZone] = React.useState<DropZone | null>(null);
  const [dragOverTabIndex, setDragOverTabIndex] = React.useState<number | null>(null);

  const clearPress = React.useCallback((current: PressState) => {
    current.cleanup();
    delete current.tab.dataset.dragged;
    if (current.tab.hasPointerCapture(current.pointerId)) {
      current.tab.releasePointerCapture(current.pointerId);
    }
    if (pressRef.current === current) pressRef.current = null;
  }, []);

  React.useEffect(() => {
    if (!draggingTab) {
      setDropZone(null);
      setDragOverTabIndex(null);
      return;
    }

    const onWindowMove = (e: PointerEvent) => {
      const hit = hitTestTabDrop(e.clientX, e.clientY);
      if (!hit || hit.targetGroupId !== groupId) {
        setDropZone(null);
        setDragOverTabIndex(null);
        return;
      }
      if (typeof hit.targetIndex === 'number') {
        setDropZone(null);
        setDragOverTabIndex(hit.targetIndex);
      } else {
        setDragOverTabIndex(null);
        setDropZone(hit.zone);
      }
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (pressRef.current) {
        if (pressRef.current.dragging) suppressClickRef.current = true;
        clearPress(pressRef.current);
      }
      setDropZone(null);
      setDragOverTabIndex(null);
      onDragStartTab(null);
    };

    window.addEventListener('pointermove', onWindowMove);
    window.addEventListener('keydown', onKeyDown);
    return () => {
      window.removeEventListener('pointermove', onWindowMove);
      window.removeEventListener('keydown', onKeyDown);
    };
  }, [draggingTab, groupId, clearPress, onDragStartTab]);

  React.useEffect(
    () => () => {
      if (!pressRef.current) return;
      const wasDragging = pressRef.current.dragging;
      clearPress(pressRef.current);
      if (wasDragging) latestRef.current.onDragStartTab(null);
    },
    [clearPress]
  );

  const onTabPointerDown = (tileId: string) => (e: React.PointerEvent<HTMLDivElement>) => {
    suppressClickRef.current = false;
    if (e.button !== 0 || pressRef.current || renamingTileId === tileId) return;
    if (e.target instanceof Element && e.target.closest('button, input')) return;

    const handleMove = (ev: PointerEvent) => {
      const current = pressRef.current;
      if (!current || ev.pointerId !== current.pointerId) return;
      if (current.dragging) return;
      if (Math.hypot(ev.clientX - current.startX, ev.clientY - current.startY) < DRAG_THRESHOLD_PX) return;
      current.dragging = true;
      try {
        current.tab.setPointerCapture(ev.pointerId);
      } catch {
        // Tab may have detached or pointer may no longer be active
      }
      current.tab.dataset.dragged = 'true';
      latestRef.current.onDragStartTab({ tileId: current.tileId, sourceGroupId: latestRef.current.groupId });
    };

    const handleUp = (ev: PointerEvent) => {
      const current = pressRef.current;
      if (!current || ev.pointerId !== current.pointerId) return;
      const wasDragging = current.dragging;
      const { tileId: draggedTileId } = current;
      clearPress(current);
      if (!wasDragging) return;
      suppressClickRef.current = true;
      const hit = hitTestTabDrop(ev.clientX, ev.clientY);
      setDropZone(null);
      setDragOverTabIndex(null);
      if (hit) {
        latestRef.current.onDropTab({
          tileId: draggedTileId,
          zone: hit.zone,
          targetIndex: hit.targetIndex,
          targetGroupId: hit.targetGroupId
        });
      }
      latestRef.current.onDragStartTab(null);
    };

    const handleCancel = (ev: PointerEvent) => {
      const current = pressRef.current;
      if (!current || ev.pointerId !== current.pointerId) return;
      const wasDragging = current.dragging;
      clearPress(current);
      setDropZone(null);
      setDragOverTabIndex(null);
      if (wasDragging) latestRef.current.onDragStartTab(null);
    };

    const cleanup = () => {
      window.removeEventListener('pointermove', handleMove);
      window.removeEventListener('pointerup', handleUp);
      window.removeEventListener('pointercancel', handleCancel);
    };

    pressRef.current = {
      tileId,
      dragging: false,
      startX: e.clientX,
      startY: e.clientY,
      tab: e.currentTarget,
      pointerId: e.pointerId,
      cleanup
    };

    window.addEventListener('pointermove', handleMove);
    window.addEventListener('pointerup', handleUp);
    window.addEventListener('pointercancel', handleCancel);
  };

  const onTabClickCapture = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!suppressClickRef.current) return;
    suppressClickRef.current = false;
    e.stopPropagation();
  };

  return {
    dropZone,
    dragOverTabIndex,
    onTabPointerDown,
    onTabClickCapture
  };
};
