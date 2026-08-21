import React from 'react';
import { Copy, Search, RefreshCw, TextCursor, ExternalLink, LoaderCircle } from 'lucide-react';

import type { LogRow } from '~/domain/interfaces/git.interface';
import type { GraphRow, GraphEdge } from '~/usecase/util/commitGraph';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import ContextMenu from '~/components/commons/ContextMenu';
import Files from '~/components/Canvas/Navigator/GitTab/History/Files';
import { commitUrl } from '~/usecase/util/gitRemote';
import { openUrl } from '~/adapter/shell/shell.client';
import { gitLogGraph, gitRemoteUrl, gitUnpushedCommits } from '~/adapter/git/git.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { isCapturing } from '~/usecase/util/keybindings';
import { graphColor, graphColorLocal, buildCommitGraph } from '~/usecase/util/commitGraph';

import styles from './styles.module.scss';

interface HistoryProps {
  root: string;
  active: string | null;
  onOpenDiff: (file: string, commit?: string) => void;
}

const PAGE = 200;
const EXIT_MS = 160;
const WIDE = 460;
const COL = 12;
const DOT = 3.5;

const laneX = (lane: number): number => 6 + lane * COL;

const author = (row: LogRow): string => (row.committer === row.author ? row.author : `${row.author}*`);

const subject = (row: LogRow): string => row.message.split('\n', 1)[0];

const refsLabel = (refs: string): string => refs.replace(/^HEAD -> /, '');

const matchRow = (row: LogRow, needle: string): boolean =>
  `${row.message} ${row.author} ${row.committer} ${row.short} ${row.refs} ${row.date}`.toLowerCase().includes(needle);

const sameLog = (prev: LogRow[], next: LogRow[]): boolean =>
  prev.length === next.length && prev.every((row, at) => row.short === next[at].short && row.refs === next[at].refs);

const startOfDay = (at: Date): number => new Date(at.getFullYear(), at.getMonth(), at.getDate()).getTime();

const stamp = (raw: string): string => {
  const at = new Date(raw.replace(' ', 'T'));
  if (Number.isNaN(at.getTime())) return raw;
  const time = raw.slice(11);
  const days = Math.round((startOfDay(new Date()) - startOfDay(at)) / 86400000);
  if (days === 0) return `Today ${time}`;
  if (days === 1) return `Yesterday ${time}`;
  return `${raw.slice(8, 10)}/${raw.slice(5, 7)}/${raw.slice(0, 4)} ${time}`;
};

const edgePath = (edge: GraphEdge, height: number): string => {
  const from = laneX(edge.fromLane);
  const to = laneX(edge.toLane);
  const mid = height / 2;

  if (edge.kind === 'through') return `M${from} 0 L${from} ${height}`;
  if (edge.fromLane === edge.toLane) {
    return edge.kind === 'in' ? `M${from} 0 L${from} ${mid}` : `M${from} ${mid} L${from} ${height}`;
  }
  if (edge.kind === 'in') return `M${from} 0 C${from} ${mid / 2} ${to} ${mid / 2} ${to} ${mid}`;
  return `M${from} ${mid} C${from} ${mid + mid / 2} ${to} ${mid + mid / 2} ${to} ${height}`;
};

interface LaneCellProps {
  row: GraphRow;
  height: number;
  head?: boolean;
  local?: boolean;
  childLocal?: boolean;
}

const isLocalEdge = (edge: GraphEdge, row: GraphRow, local?: boolean, childLocal?: boolean): boolean => {
  if (edge.fromLane !== row.lane && edge.toLane !== row.lane) return false;
  return edge.kind === 'in' ? Boolean(childLocal) : Boolean(local);
};

const LaneCell = ({ row, height, head, local, childLocal }: LaneCellProps) => {
  const width = row.width * COL;
  const color = local ? graphColorLocal(row.color) : graphColor(row.color);
  const x = laneX(row.lane);
  return (
    <svg className={styles.graph} width={width} height={height} viewBox={`0 0 ${width} ${height}`}>
      {row.edges.map((edge) => (
        <path
          key={`${edge.kind}-${edge.fromLane}-${edge.toLane}`}
          d={edgePath(edge, height)}
          stroke={isLocalEdge(edge, row, local, childLocal) ? graphColorLocal(edge.color) : graphColor(edge.color)}
          strokeWidth={1.5}
          strokeLinecap="round"
          fill="none"
        />
      ))}
      {head && <circle cx={x} cy={height / 2} r={DOT + 1.6} stroke={color} strokeWidth={1.2} fill="none" />}
      <circle cx={x} cy={height / 2} r={DOT} fill={color} />
    </svg>
  );
};

const History = ({ root, active, onOpenDiff }: HistoryProps) => {
  const rootRef = React.useRef<HTMLDivElement>(null);
  const listRef = React.useRef<HTMLDivElement>(null);
  const [rows, setRows] = React.useState<LogRow[] | null>(null);
  const [limit, setLimit] = React.useState(PAGE);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [wide, setWide] = React.useState(true);
  const [remote, setRemote] = React.useState<string | null>(null);
  const [local, setLocal] = React.useState<Set<string>>(() => new Set());
  const [menu, setMenu] = React.useState<{ x: number; y: number; row: LogRow } | null>(null);
  const [filter, setFilter] = React.useState('');
  const [picked, setPicked] = React.useState<LogRow | null>(null);
  const [exiting, setExiting] = React.useState(false);
  const exitTimer = React.useRef(0);

  const fetchLog = React.useCallback(
    (quiet: boolean) => {
      if (!quiet) setBusy(true);
      gitUnpushedCommits(root)
        .then((ahead) => setLocal(new Set(ahead.map((entry) => entry.short))))
        .catch(() => setLocal(new Set()));
      gitLogGraph(root, limit)
        .then((next) => {
          setRows((prev) => (prev && sameLog(prev, next) ? prev : next));
          setError(null);
        })
        .catch((err: unknown) => {
          if (!quiet) setError(typeof err === 'string' ? err : String(err));
        })
        .finally(() => {
          if (!quiet) setBusy(false);
        });
    },
    [root, limit]
  );

  const load = React.useCallback(() => fetchLog(false), [fetchLog]);

  React.useEffect(load, [load]);

  React.useEffect(() => {
    const timer = window.setInterval(() => fetchLog(true), 5000);
    return () => window.clearInterval(timer);
  }, [fetchLog]);

  React.useEffect(() => {
    gitRemoteUrl(root)
      .then(setRemote)
      .catch(() => setRemote(null));
  }, [root]);

  React.useEffect(() => {
    const el = listRef.current;
    if (!el) return;
    const observer = new ResizeObserver(([entry]) => setWide(entry.contentRect.width >= WIDE));
    observer.observe(el);
    return () => observer.disconnect();
  }, [rows]);

  const graph = React.useMemo(() => (rows ? buildCommitGraph(rows) : []), [rows]);

  const headAt = React.useMemo(() => (rows ? rows.findIndex((row) => /(^|, )HEAD( ->|,|$)/.test(row.refs)) : -1), [rows]);

  const needle = filter.trim().toLowerCase();

  const shown = React.useMemo(() => {
    if (!rows) return [];
    const all = rows.map((row, at) => ({ row, at }));
    if (!needle) return all;
    return all.filter(({ row }) => matchRow(row, needle));
  }, [rows, needle]);

  const closePanel = React.useCallback(() => {
    setPicked((prev) => {
      if (!prev) return prev;
      setExiting(true);
      exitTimer.current = window.setTimeout(() => {
        setPicked(null);
        setExiting(false);
      }, EXIT_MS);
      return prev;
    });
  }, []);

  React.useEffect(() => () => window.clearTimeout(exitTimer.current), []);

  React.useEffect(() => {
    if (!picked) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || isCapturing() || active) return;
      const focused = document.activeElement;
      if (focused && focused !== document.body && !rootRef.current?.contains(focused)) return;
      e.preventDefault();
      e.stopPropagation();
      closePanel();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [picked, active, closePanel]);

  const pick = (row: LogRow) => {
    listRef.current?.focus({ preventScroll: true });
    window.clearTimeout(exitTimer.current);
    setExiting(false);
    setPicked((prev) => (prev?.short === row.short ? prev : row));
  };

  const onFilter = (e: React.ChangeEvent<HTMLInputElement>) => setFilter(e.target.value);

  const more = () => setLimit((prev) => prev + PAGE);

  const closeMenu = () => setMenu(null);

  const openMenu = (e: React.MouseEvent, row: LogRow) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, row });
  };

  const menuItems = (row: LogRow): ContextMenuEntry[] => {
    const url = remote ? commitUrl(remote, row.short) : null;
    const items: ContextMenuEntry[] = [
      {
        label: 'Copy hash',
        icon: <Copy size={15} strokeWidth={1.75} />,
        onSelect: () => writeClipboard(row.short)
      },
      {
        label: 'Copy commit message',
        icon: <TextCursor size={15} strokeWidth={1.75} />,
        onSelect: () => writeClipboard(row.message)
      }
    ];
    if (url) {
      items.push('separator', {
        label: 'Open in browser',
        icon: <ExternalLink size={15} strokeWidth={1.75} />,
        onSelect: () => openUrl(url)
      });
    }
    return items;
  };

  const height = wide ? 24 : 38;

  if (error) return <div className={styles.notice}>{error}</div>;

  if (!rows)
    return (
      <div className={styles.notice}>
        <LoaderCircle size={16} strokeWidth={2} className={styles.spinning} />
      </div>
    );

  return (
    <div ref={rootRef} className={styles.root}>
      <div className={styles.toolbar}>
        <button className={styles.tool} onClick={load} disabled={busy} title="Refresh" aria-label="Refresh">
          <RefreshCw size={12} strokeWidth={2} className={busy ? styles.spinning : undefined} />
        </button>
        <div className={styles.filter}>
          <Search size={12} strokeWidth={2} />
          <input value={filter} onChange={onFilter} placeholder="Filter" spellCheck={false} />
        </div>
        <span className={styles.count}>{shown.length === 1 ? '1 commit' : `${shown.length} commits`}</span>
      </div>

      <div ref={listRef} className={styles.list} tabIndex={-1}>
        {shown.map(({ row, at }) => {
          const menuAt = (e: React.MouseEvent) => openMenu(e, row);
          const select = () => pick(row);
          return (
            <div
              key={row.short}
              className={styles.row}
              style={{ height, paddingLeft: needle ? 8 : undefined }}
              onClick={select}
              onContextMenu={menuAt}
              data-picked={row.short === picked?.short || undefined}
              data-wide={wide || undefined}
              data-merge={row.parents.length > 1 || undefined}
              data-head={at === headAt || undefined}
            >
              {!needle && (
                <LaneCell
                  row={graph[at]}
                  height={height}
                  head={at === headAt}
                  local={local.has(row.short)}
                  childLocal={at > 0 && local.has(rows[at - 1].short)}
                />
              )}
              <div className={styles.body}>
                <span className={styles.subject} title={row.message}>
                  {subject(row)}
                </span>
                <span className={styles.side}>
                  {row.refs && (
                    <span className={styles.refs} title={row.refs}>
                      {refsLabel(row.refs)}
                    </span>
                  )}
                  <span className={styles.author} title={author(row)}>
                    {author(row)}
                  </span>
                  <span className={styles.date}>{stamp(row.date)}</span>
                </span>
              </div>
            </div>
          );
        })}
        {shown.length === 0 && <div className={styles.notice}>No matching commits</div>}
        {rows.length >= limit && (
          <button className={styles.more} onClick={more} disabled={busy}>
            Load more
          </button>
        )}
      </div>

      {picked && (
        <Files root={root} commit={picked} active={active} exiting={exiting} onOpenDiff={onOpenDiff} onClose={closePanel} />
      )}

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.row)} onClose={closeMenu} />}
    </div>
  );
};

export default History;
