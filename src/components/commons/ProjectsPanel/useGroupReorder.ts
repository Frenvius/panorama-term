import type { DropPlace } from '~/usecase/util/projectOrder';
import type { ProjectGroup } from '~/domain/interfaces/projectGroup.interface';

import React from 'react';

const DRAG_THRESHOLD_PX = 4;

export interface GroupDrop {
  group: ProjectGroup;
  place: DropPlace;
}

interface GroupReorderOpts {
  enabled: boolean;
  groups: ProjectGroup[];
  onReorder: (from: ProjectGroup, to: ProjectGroup, place: DropPlace) => void;
}

interface Press {
  from: ProjectGroup;
  x: number;
  y: number;
  dragging: boolean;
}

interface DropAtOpts {
  x: number;
  y: number;
  from: ProjectGroup;
  groups: ProjectGroup[];
}

const dropAt = ({ x, y, from, groups }: DropAtOpts): GroupDrop | null => {
  const row = document.elementFromPoint(x, y)?.closest<HTMLElement>('[data-group-key]');
  if (!row) return null;
  const group = groups.find((g) => g.key === row.dataset.groupKey);
  if (!group || group.kind !== from.kind || group.key === from.key) return null;
  const rect = row.getBoundingClientRect();
  return { group, place: y < rect.top + rect.height / 2 ? 'before' : 'after' };
};

export const useGroupReorder = ({ enabled, groups, onReorder }: GroupReorderOpts) => {
  const [drop, setDrop] = React.useState<GroupDrop | null>(null);
  const press = React.useRef<Press | null>(null);
  const suppressClick = React.useRef(false);

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    suppressClick.current = false;
    if (!enabled || e.button !== 0 || !(e.target instanceof Element) || e.target.closest('button, input')) return;
    const row = e.target.closest<HTMLElement>('[data-group-key]');
    const from = groups.find((g) => g.key === row?.dataset.groupKey);
    if (!from || from.kind === 'loose') return;
    press.current = { from, x: e.clientX, y: e.clientY, dragging: false };
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const current = press.current;
    if (!current) return;
    if (!current.dragging) {
      if (Math.hypot(e.clientX - current.x, e.clientY - current.y) < DRAG_THRESHOLD_PX) return;
      current.dragging = true;
      e.currentTarget.setPointerCapture(e.pointerId);
    }
    const next = dropAt({ x: e.clientX, y: e.clientY, from: current.from, groups });
    setDrop((prev) => (prev?.group.key === next?.group.key && prev?.place === next?.place ? prev : next));
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    const current = press.current;
    press.current = null;
    setDrop(null);
    if (!current?.dragging) return;
    suppressClick.current = true;
    const target = dropAt({ x: e.clientX, y: e.clientY, from: current.from, groups });
    if (target) onReorder(current.from, target.group, target.place);
  };

  const handlePointerCancel = () => {
    press.current = null;
    setDrop(null);
  };

  const handleClickCapture = (e: React.MouseEvent<HTMLDivElement>) => {
    if (!suppressClick.current) return;
    suppressClick.current = false;
    e.stopPropagation();
  };

  return {
    drop,
    bodyHandlers: {
      onPointerUp: handlePointerUp,
      onPointerDown: handlePointerDown,
      onPointerMove: handlePointerMove,
      onClickCapture: handleClickCapture,
      onPointerCancel: handlePointerCancel
    }
  };
};
