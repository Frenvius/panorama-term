import type { Frame } from '~/domain/interfaces/workspace.interface';

export type DropPlace = 'before' | 'after';

interface MoveListItemOpts {
  items: string[];
  from: string;
  to: string;
  place: DropPlace;
}

const moveListItem = ({ items, from, to, place }: MoveListItemOpts): string[] => {
  if (from === to || !items.includes(from) || !items.includes(to)) return items;
  const rest = items.filter((item) => item !== from);
  const anchor = rest.indexOf(to) + (place === 'after' ? 1 : 0);
  return [...rest.slice(0, anchor), from, ...rest.slice(anchor)];
};

interface MoveProjectOpts {
  projects: string[];
  from: string;
  to: string;
  place: DropPlace;
}

export const moveProject = ({ projects, from, to, place }: MoveProjectOpts): string[] =>
  moveListItem({ items: projects, from, to, place });

interface MoveFrameOrderOpts {
  frames: Frame[];
  from: string;
  to: string;
  place: DropPlace;
}

export const moveFrameOrder = ({ frames, from, to, place }: MoveFrameOrderOpts): Frame[] => {
  const byId = new Map(frames.map((frame) => [frame.id, frame]));
  const ids = moveListItem({ items: frames.map((frame) => frame.id), from, to, place });
  return ids.map((id) => byId.get(id)!);
};
