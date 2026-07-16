import React from 'react';
import { Copy, RefreshCw, TextCursor, ExternalLink, LoaderCircle } from 'lucide-react';

import type { LogRow } from '~/domain/interfaces/git.interface';
import type { GraphRow, GraphEdge } from '~/usecase/util/commitGraph';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import ContextMenu from '~/components/commons/ContextMenu';
import { commitUrl } from '~/usecase/util/gitRemote';
import { openUrl } from '~/adapter/shell/shell.client';
import { gitLogGraph, gitRemoteUrl } from '~/adapter/git/git.client';
import { writeClipboard } from '~/adapter/clipboard/clipboard.client';
import { graphColor, buildCommitGraph } from '~/usecase/util/commitGraph';

import styles from './styles.module.scss';

interface HistoryProps {
  root: string;
}

const PAGE = 200;
const WIDE = 460;
const COL = 12;
const DOT = 3.5;

const laneX = (lane: number): number => 6 + lane * COL;

const author = (row: LogRow): string => (row.committer === row.author ? row.author : `${row.author}*`);

const subject = (row: LogRow): string => row.message.split('\n', 1)[0];

const refsLabel = (refs: string): string => refs.replace(/^HEAD -> /, '');

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
}

const LaneCell = ({ row, height }: LaneCellProps) => {
  const width = row.width * COL;
  return (
    <svg className={styles.graph} width={width} height={height} viewBox={`0 0 ${width} ${height}`}>
      {row.edges.map((edge) => (
        <path
          key={`${edge.kind}-${edge.fromLane}-${edge.toLane}`}
          d={edgePath(edge, height)}
          stroke={graphColor(edge.color)}
          strokeWidth={1.5}
          strokeLinecap="round"
          fill="none"
        />
      ))}
      <circle cx={laneX(row.lane)} cy={height / 2} r={DOT} fill={graphColor(row.color)} />
    </svg>
  );
};

const History = ({ root }: HistoryProps) => {
  const listRef = React.useRef<HTMLDivElement>(null);
  const [rows, setRows] = React.useState<LogRow[] | null>(null);
  const [limit, setLimit] = React.useState(PAGE);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [wide, setWide] = React.useState(true);
  const [remote, setRemote] = React.useState<string | null>(null);
  const [menu, setMenu] = React.useState<{ x: number; y: number; row: LogRow } | null>(null);

  const load = React.useCallback(() => {
    setBusy(true);
    gitLogGraph(root, limit)
      .then((next) => {
        setRows(next);
        setError(null);
      })
      .catch((err: unknown) => setError(typeof err === 'string' ? err : String(err)))
      .finally(() => setBusy(false));
  }, [root, limit]);

  React.useEffect(load, [load]);

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
    <div className={styles.root}>
      <div className={styles.toolbar}>
        <button className={styles.tool} onClick={load} disabled={busy} title="Refresh" aria-label="Refresh">
          <RefreshCw size={12} strokeWidth={2} className={busy ? styles.spinning : undefined} />
        </button>
        <span className={styles.count}>{rows.length === 1 ? '1 commit' : `${rows.length} commits`}</span>
      </div>

      <div ref={listRef} className={styles.list}>
        {rows.map((row, at) => {
          const menuAt = (e: React.MouseEvent) => openMenu(e, row);
          return (
            <div
              key={row.short}
              className={styles.row}
              style={{ height }}
              onContextMenu={menuAt}
              data-wide={wide || undefined}
              data-merge={row.parents.length > 1 || undefined}
            >
              <LaneCell row={graph[at]} height={height} />
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
        {rows.length >= limit && (
          <button className={styles.more} onClick={more} disabled={busy}>
            Load more
          </button>
        )}
      </div>

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.row)} onClose={closeMenu} />}
    </div>
  );
};

export default History;
