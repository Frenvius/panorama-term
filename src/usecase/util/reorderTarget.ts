export interface ReorderRect {
  top: number;
  height: number;
}

interface ReorderTargetOpts {
  centerY: number;
  rects: ReorderRect[];
  from: number;
}

export const reorderTargetIndex = ({ centerY, rects, from }: ReorderTargetOpts): number => {
  let target = from;
  rects.forEach((rect, index) => {
    const middle = rect.top + rect.height / 2;
    if (index < from && centerY < middle) target = Math.min(target, index);
    if (index > from && centerY > middle) target = Math.max(target, index);
  });
  return target;
};
