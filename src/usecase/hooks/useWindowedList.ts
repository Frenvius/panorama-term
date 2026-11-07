import React from 'react';

interface WindowedListOptions {
  count: number;
  rowHeight: number;
  overscan?: number;
}

export const useWindowedList = ({ count, rowHeight, overscan = 10 }: WindowedListOptions) => {
  const [el, attach] = React.useState<HTMLElement | null>(null);
  const [view, setView] = React.useState({ top: 0, height: 0 });

  React.useEffect(() => {
    if (!el) return;
    const measure = () =>
      setView((prev) =>
        prev.top === el.scrollTop && prev.height === el.clientHeight ? prev : { top: el.scrollTop, height: el.clientHeight }
      );
    measure();
    el.addEventListener('scroll', measure, { passive: true });
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => {
      el.removeEventListener('scroll', measure);
      observer.disconnect();
    };
  }, [el]);

  const end = Math.min(count, Math.ceil((view.top + view.height) / rowHeight) + overscan);
  const start = Math.min(end, Math.max(0, Math.floor(view.top / rowHeight) - overscan));

  const scrollToIndex = React.useCallback(
    (index: number) => {
      if (!el) return;
      const top = index * rowHeight;
      if (top < el.scrollTop) el.scrollTop = top;
      else if (top + rowHeight > el.scrollTop + el.clientHeight) el.scrollTop = top + rowHeight - el.clientHeight;
    },
    [el, rowHeight]
  );

  return { attach, start, end, before: start * rowHeight, after: (count - end) * rowHeight, scrollToIndex };
};
