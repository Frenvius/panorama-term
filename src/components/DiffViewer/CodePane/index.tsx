import type { ThemedToken } from 'shiki';
import type {
  PaneApi,
  DiffSide,
  ChunkKind,
  DiffChunk,
  VisualRow,
  IntraLineRange
} from '~/domain/interfaces/diff.interface';

import React from 'react';

import { highlight } from '~/usecase/service/highlight';
import { lineKinds, firstAfter, LINE_HEIGHT, SKIP_HEIGHT } from '~/usecase/util/diff';

import styles from './styles.module.scss';

interface CodePaneProps {
  code: string;
  lang: string;
  side: DiffSide;
  chunks: DiffChunk[];
  current: DiffChunk | null;
  rows: VisualRow[];
  ranges: Map<number, IntraLineRange[]> | null;
  overrides: Map<number, ChunkKind> | null;
  onApi: (api: PaneApi | null) => void;
  onExpand: (index: number) => void;
}

const TAB_SIZE = 4;
const OVERSCAN = 6;

const rangeAt = (col: number, ranges: IntraLineRange[]): IntraLineRange | undefined =>
  ranges.find((r) => col >= r.startCol && col < r.endCol);

const visualLength = (line: string): number => {
  let n = 0;
  for (const ch of line) n += ch === '\t' ? TAB_SIZE - (n % TAB_SIZE) : 1;
  return n;
};

const paintToken = (
  text: string,
  color: string | undefined,
  italic: boolean,
  start: number,
  ranges: IntraLineRange[] | null,
  key: string
): React.ReactNode[] => {
  const style = { color, fontStyle: italic ? ('italic' as const) : undefined };
  if (!ranges || ranges.length === 0) {
    return [
      <span key={key} style={style}>
        {text}
      </span>
    ];
  }

  const end = start + text.length;
  const cuts = new Set<number>([start, end]);
  for (const r of ranges) {
    if (r.startCol > start && r.startCol < end) cuts.add(r.startCol);
    if (r.endCol > start && r.endCol < end) cuts.add(r.endCol);
  }

  const sorted = [...cuts].sort((a, b) => a - b);
  const out: React.ReactNode[] = [];

  for (let i = 0; i < sorted.length - 1; i++) {
    const a = sorted[i];
    const b = sorted[i + 1];
    if (a === b) continue;
    const hit = rangeAt(a, ranges);
    out.push(
      <span key={`${key}-${i}`} className={hit ? styles[`hl-${hit.kind}`] : undefined} style={style}>
        {text.slice(a - start, b - start)}
      </span>
    );
  }

  return out;
};

const CodePane = ({ code, lang, side, chunks, current, rows, ranges, overrides, onApi, onExpand }: CodePaneProps) => {
  const elRef = React.useRef<HTMLDivElement>(null);
  const scrollCbs = React.useRef<Set<() => void>>(new Set());
  const resizeCbs = React.useRef<Set<() => void>>(new Set());
  const [tokens, setTokens] = React.useState<ThemedToken[][] | null>(null);

  const raw = React.useMemo(() => code.split('\n'), [code]);
  const kinds = React.useMemo(() => lineKinds(chunks, side), [chunks, side]);

  React.useEffect(() => {
    let alive = true;
    highlight(code, lang).then((lines) => alive && setTokens(lines));
    return () => {
      alive = false;
    };
  }, [code, lang]);

  React.useEffect(() => {
    const el = elRef.current;
    if (!el) return;
    el.scrollLeft = side === 'orig' ? -el.scrollWidth : 0;
  }, [code, side, tokens]);

  const tops = React.useMemo(() => {
    const out = new Array<number>(rows.length + 1);
    let y = 0;
    rows.forEach((row, i) => {
      out[i] = y;
      y += row.kind === 'skip' ? SKIP_HEIGHT : LINE_HEIGHT;
    });
    out[rows.length] = y;
    return out;
  }, [rows]);

  const lineToRow = React.useMemo(() => {
    const map = new Map<number, number>();
    rows.forEach((row, i) => {
      if (row.kind === 'line') map.set(row.lineNumber, i);
      else for (let l = row.firstHidden; l <= row.lastHidden; l++) map.set(l, i);
    });
    return map;
  }, [rows]);

  const api = React.useMemo<PaneApi>(
    () => ({
      getElement: () => elRef.current,
      getScrollTop: () => elRef.current?.scrollTop ?? 0,
      setScrollTop: (y) => {
        if (elRef.current) elRef.current.scrollTop = y;
      },
      getTopForLine: (line) => {
        const idx = lineToRow.get(line);
        return idx === undefined ? (tops[rows.length] ?? 0) : tops[idx];
      },
      getRowTop: (index) => tops[index] ?? 0,
      getRows: () => rows,
      onScroll: (cb) => {
        scrollCbs.current.add(cb);
        return () => scrollCbs.current.delete(cb) as unknown as void;
      },
      onResize: (cb) => {
        resizeCbs.current.add(cb);
        return () => resizeCbs.current.delete(cb) as unknown as void;
      }
    }),
    [rows, tops, lineToRow]
  );

  React.useEffect(() => {
    onApi(api);
  }, [api, onApi]);

  React.useEffect(() => {
    const el = elRef.current;
    if (!el) return;

    const onScroll = () => scrollCbs.current.forEach((cb) => cb());
    const ro = new ResizeObserver(() => resizeCbs.current.forEach((cb) => cb()));

    el.addEventListener('scroll', onScroll, { passive: true });
    ro.observe(el);

    return () => {
      el.removeEventListener('scroll', onScroll);
      ro.disconnect();
    };
  }, []);

  const [span, setSpan] = React.useState({ from: 0, to: 0 });

  React.useEffect(() => {
    let raf = 0;

    const compute = () => {
      const el = elRef.current;
      if (!el) return;

      const top = el.scrollTop - LINE_HEIGHT * OVERSCAN;
      const bottom = el.scrollTop + el.clientHeight + LINE_HEIGHT * OVERSCAN;
      const topAt = (i: number) => tops[i];
      setSpan({ from: firstAfter(topAt, tops.length, top), to: firstAfter(topAt, tops.length, bottom) + 1 });
    };

    const schedule = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        compute();
      });
    };

    const el = elRef.current;
    el?.addEventListener('scroll', schedule, { passive: true });
    const ro = new ResizeObserver(schedule);
    if (el) ro.observe(el);
    schedule();

    return () => {
      if (raf) cancelAnimationFrame(raf);
      el?.removeEventListener('scroll', schedule);
      ro.disconnect();
    };
  }, [tops]);

  const width = React.useMemo(() => raw.reduce((n, line) => Math.max(n, visualLength(line)), 0), [raw]);

  return (
    <div ref={elRef} className={styles.pane} data-side={side}>
      <div
        className={styles.content}
        style={{ height: tops[rows.length] ?? 0, minWidth: `calc(${width}ch + 24px)` }}
      >
        {rows.slice(span.from, span.to).map((row, offset) => {
          const i = span.from + offset;
          if (row.kind === 'skip') {
            return (
              <button
                key={`s${i}`}
                className={styles.skip}
                style={{ top: tops[i], height: SKIP_HEIGHT }}
                onClick={() => onExpand(row.index)}
                data-tooltip={`Show ${row.count} hidden lines`}
                aria-label={`Show ${row.count} hidden lines`}
              />
            );
          }

          const line = row.lineNumber;
          const from = current ? (side === 'orig' ? current.origStart : current.modStart) : 0;
          const to = current ? (side === 'orig' ? current.origEnd : current.modEnd) : 0;
          const active = current !== null && line >= from && line < to;
          const override = overrides?.get(line);
          const kind = override ?? kinds.get(line);
          const marks = override ? null : (ranges?.get(line) ?? null);
          const lineTokens = tokens?.[line - 1];

          const nodes: React.ReactNode[] = [];
          let col = 0;

          for (const [j, token] of (lineTokens ?? []).entries()) {
            nodes.push(...paintToken(token.content, token.color, token.fontStyle === 1, col, marks, `t${j}`));
            col += token.content.length;
          }

          const body = lineTokens ? nodes : (raw[line - 1] ?? '');

          return (
            <pre
              key={`l${i}`}
              className={kind ? `${styles.line} ${styles[kind]}` : styles.line}
              data-current={active}
              style={{ top: tops[i], height: LINE_HEIGHT, lineHeight: `${LINE_HEIGHT}px` }}
            >
              {body}
            </pre>
          );
        })}
      </div>
    </div>
  );
};

export default CodePane;
