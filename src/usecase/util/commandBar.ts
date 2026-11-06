import type {
  CommandBarRow,
  CommandBarItem,
  CommandBarKind,
  CommandBarSection
} from '~/domain/interfaces/commandBar.interface';

export const COMMAND_PREFIX = '>';

const SECTION_CAP = 12;

const SECTION_ORDER: CommandBarKind[] = ['tile', 'frame', 'tab', 'workspace', 'command', 'setting'];

const SECTION_TITLES: Record<CommandBarKind, string> = {
  tab: 'Tabs',
  tile: 'Tiles',
  frame: 'Frames',
  setting: 'Settings',
  command: 'Commands',
  workspace: 'Workspaces'
};

const ACTION_KINDS = new Set<CommandBarKind>(['command', 'setting']);

const range = (from: number, length: number): number[] => Array.from({ length }, (_, i) => from + i);

export const matchScore = (text: string, needle: string): { rank: number; hits: number[] } | null => {
  const hay = text.toLowerCase();
  const hits: number[] = [];
  let last = -1;
  let rank = 0;
  for (const ch of needle) {
    const at = hay.indexOf(ch, last + 1);
    if (at === -1) return null;
    const gap = at - last - 1;
    rank += gap === 0 ? 15 : Math.max(2, 12 - gap);
    hits.push(at);
    last = at;
  }
  const sub = hay.indexOf(needle);
  if (sub === -1) return { rank, hits };
  return { rank: rank + (sub === 0 ? 20 : 12), hits: range(sub, needle.length) };
};

const searchText = (item: CommandBarItem): string => [item.label, item.scope, item.detail].filter(Boolean).join(' ');

const rankRow = (item: CommandBarItem, needle: string): CommandBarRow | null => {
  const match = matchScore(searchText(item), needle);
  if (!match) return null;
  const inLabel = match.hits.every((i) => i < item.label.length);
  return { item, rank: match.rank, hits: inLabel ? match.hits : [] };
};

const asRows = (items: CommandBarItem[]): CommandBarRow[] => items.map((item) => ({ item, rank: 0, hits: [] }));

const groupByKind = (rows: CommandBarRow[]): CommandBarSection[] =>
  SECTION_ORDER.map((kind) => ({ kind, title: SECTION_TITLES[kind], rows: rows.filter((r) => r.item.kind === kind) })).filter(
    (s) => s.rows.length > 0
  );

const topRank = (section: CommandBarSection): number => section.rows[0]?.rank ?? 0;

export const buildSections = (items: CommandBarItem[], query: string): CommandBarSection[] => {
  const actionsOnly = query.startsWith(COMMAND_PREFIX);
  const needle = (actionsOnly ? query.slice(COMMAND_PREFIX.length) : query).trim().toLowerCase();
  const pool = actionsOnly ? items.filter((i) => ACTION_KINDS.has(i.kind)) : items;
  if (!needle && actionsOnly) return groupByKind(asRows(pool));
  if (!needle) {
    const recent = pool.filter((i) => i.kind === 'tile' && !i.scope);
    return recent.length ? [{ kind: 'recent', title: 'Recent', rows: asRows(recent) }] : [];
  }
  const ranked = pool
    .map((item) => rankRow(item, needle))
    .filter((row): row is CommandBarRow => row !== null)
    .sort((a, b) => b.rank - a.rank);
  return groupByKind(ranked)
    .map((s) => ({ ...s, rows: s.rows.slice(0, SECTION_CAP) }))
    .sort((a, b) => topRank(b) - topRank(a));
};
