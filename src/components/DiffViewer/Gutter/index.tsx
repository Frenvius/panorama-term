import type { PaneApi, DiffSide, ChunkKind, DiffChunk } from '~/domain/interfaces/diff.interface';

import React from 'react';

import { lineKinds, firstAfter, LINE_HEIGHT, SKIP_HEIGHT } from '~/usecase/util/diff';

import styles from './styles.module.scss';

interface GutterProps {
  api: PaneApi;
  side: DiffSide;
  chunks: DiffChunk[];
}

interface GutterRow {
  key: string;
  y: number;
  height: number;
  line: number | null;
  kind: ChunkKind | null;
}

const Gutter = ({ api, side, chunks }: GutterProps) => {
  const [rows, setRows] = React.useState<GutterRow[]>([]);
  const boxRef = React.useRef<HTMLDivElement>(null);

  const kinds = React.useMemo(() => lineKinds(chunks, side), [chunks, side]);

  React.useEffect(() => {
    let raf = 0;

    const compute = () => {
      const scrollTop = api.getScrollTop();
      const visual = api.getRows();
      const height = boxRef.current?.clientHeight ?? 800;
      const top = scrollTop - LINE_HEIGHT * 2;
      const bottom = scrollTop + height + LINE_HEIGHT * 2;
      const out: GutterRow[] = [];

      const start = firstAfter(api.getRowTop, visual.length + 1, top);

      for (let i = start; i < visual.length; i++) {
        const row = visual[i];
        const y = api.getRowTop(i);
        const h = row.kind === 'skip' ? SKIP_HEIGHT : LINE_HEIGHT;
        if (y + h < top) continue;
        if (y > bottom) break;

        out.push(
          row.kind === 'skip'
            ? { key: `s${i}`, y: y - scrollTop, height: h, line: null, kind: null }
            : {
                key: `l${row.lineNumber}`,
                y: y - scrollTop,
                height: h,
                line: row.lineNumber,
                kind: kinds.get(row.lineNumber) ?? null
              }
        );
      }

      setRows(out);
    };

    const schedule = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        compute();
      });
    };

    const offScroll = api.onScroll(schedule);
    const offResize = api.onResize(schedule);
    const ro = new ResizeObserver(schedule);
    if (boxRef.current) ro.observe(boxRef.current);
    schedule();

    return () => {
      if (raf) cancelAnimationFrame(raf);
      offScroll();
      offResize();
      ro.disconnect();
    };
  }, [api, kinds]);

  return (
    <div ref={boxRef} className={styles.gutter} data-side={side} aria-hidden>
      {rows.map((row) => (
        <div
          key={row.key}
          className={row.kind ? `${styles.row} ${styles[row.kind]}` : styles.row}
          style={{ top: row.y, height: row.height, lineHeight: `${row.height}px` }}
        >
          {row.line}
        </div>
      ))}
    </div>
  );
};

export default Gutter;
