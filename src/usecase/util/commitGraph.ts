import type { LogRow } from '~/domain/interfaces/git.interface';

export interface GraphEdge {
  fromLane: number;
  toLane: number;
  kind: 'through' | 'in' | 'out';
  color: number;
}

export interface GraphRow {
  lane: number;
  color: number;
  edges: GraphEdge[];
  width: number;
}

const SATURATION = 27.1;
const LIGHTNESS = 52;

const javaHash = (text: string): number => {
  let hash = 0;
  for (let at = 0; at < text.length; at += 1) hash = (Math.imul(31, hash) + text.charCodeAt(at)) | 0;
  return hash;
};

const rangeFix = (n: number): number => Math.abs(n % 100) + 70;

const hue = (color: number): number => {
  const r = rangeFix((Math.imul(color, 200) + 30) | 0);
  const g = rangeFix((Math.imul(color, 130) + 50) | 0);
  const b = rangeFix((Math.imul(color, 90) + 100) | 0);

  const top = Math.max(r, g, b);
  const low = Math.min(r, g, b);
  if (top === low) return 0;

  const span = top - low;
  const raw = top === r ? (g - b) / span : top === g ? (b - r) / span + 2 : (r - g) / span + 4;
  return ((raw * 60) % 360 + 360) % 360;
};

export const graphColor = (color: number): string => `hsl(${hue(color)} ${SATURATION}% ${LIGHTNESS}%)`;

export const graphColorLocal = (color: number): string => `hsl(${(hue(color) + 180) % 360} 72% 62%)`;

const headRef = (refs: string): string | null => {
  for (const raw of refs.split(',')) {
    const name = raw.trim().replace(/^HEAD -> /, '');
    if (!name || name === 'HEAD' || name.startsWith('tag: ')) continue;
    return name;
  }
  return null;
};

const firstFree = (lanes: (string | null)[]): number => {
  const at = lanes.indexOf(null);
  if (at !== -1) return at;
  lanes.push(null);
  return lanes.length - 1;
};

const used = (lanes: (string | null)[]): number => {
  let last = -1;
  for (let at = 0; at < lanes.length; at += 1) {
    if (lanes[at] !== null) last = at;
  }
  return last + 1;
};

export const buildCommitGraph = (rows: LogRow[]): GraphRow[] => {
  const lanes: (string | null)[] = [];
  const colors: number[] = [];
  const out: GraphRow[] = [];

  for (const row of rows) {
    const incoming: number[] = [];
    for (let at = 0; at < lanes.length; at += 1) {
      if (lanes[at] === row.short) incoming.push(at);
    }

    const lane = incoming.length > 0 ? incoming[0] : firstFree(lanes);
    if (incoming.length === 0) {
      const ref = headRef(row.refs);
      colors[lane] = ref ? javaHash(ref) : lane;
    }

    const before = [...lanes];
    const edges: GraphEdge[] = [];

    for (const at of incoming) {
      if (at === lane) continue;
      edges.push({ fromLane: at, toLane: lane, kind: 'in', color: colors[at] });
      lanes[at] = null;
    }
    if (incoming.includes(lane)) edges.push({ fromLane: lane, toLane: lane, kind: 'in', color: colors[lane] });

    lanes[lane] = row.parents[0] ?? null;
    if (lanes[lane]) edges.push({ fromLane: lane, toLane: lane, kind: 'out', color: colors[lane] });

    for (const parent of row.parents.slice(1)) {
      let at = lanes.indexOf(parent);
      if (at === -1) {
        at = firstFree(lanes);
        lanes[at] = parent;
        colors[at] = at;
      }
      edges.push({ fromLane: lane, toLane: at, kind: 'out', color: colors[at] });
    }

    for (let at = 0; at < before.length; at += 1) {
      if (before[at] === null || incoming.includes(at)) continue;
      if (before[at] === lanes[at]) edges.push({ fromLane: at, toLane: at, kind: 'through', color: colors[at] });
    }

    out.push({ lane, color: colors[lane], edges, width: Math.max(used(before), used(lanes), lane + 1) });
  }

  return out;
};
