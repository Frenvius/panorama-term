import { diffChars, diffLines, diffArrays } from 'diff';

import type {
  DiffSide,
  ChunkKind,
  DiffChunk,
  VisualRow,
  UnifiedRow,
  PaneApi,
  ScrollAnchor,
  HighlightMode,
  UnifiedVisual,
  IntraLineRange,
  IntraLineHighlights
} from '~/domain/interfaces/diff.interface';

const ALIGN_MIN_SIMILARITY = 0.25;
const ALIGN_MAX_CELLS = 40000;

export const LINE_HEIGHT = 18;
export const SKIP_HEIGHT = 6;
export const COLLAPSE_CONTEXT = 3;

export const langOf = (path: string): string => path.slice(path.lastIndexOf('.') + 1).toLowerCase();

export const computeDiff = (original: string, modified: string, ignoreWhitespace = false): DiffChunk[] => {
  const parts = diffLines(original, modified, { ignoreWhitespace });
  const chunks: DiffChunk[] = [];

  let origLine = 1;
  let modLine = 1;
  let i = 0;

  while (i < parts.length) {
    const part = parts[i];
    const count = part.count ?? 0;

    if (!part.added && !part.removed) {
      origLine += count;
      modLine += count;
      i++;
      continue;
    }

    const origStart = origLine;
    const modStart = modLine;
    let hasAdded = false;
    let hasRemoved = false;
    let origLines = 0;
    let modLines = 0;

    while (i < parts.length && (parts[i].added || parts[i].removed)) {
      const p = parts[i];
      const c = p.count ?? 0;
      if (p.added) {
        hasAdded = true;
        modLines += c;
      } else {
        hasRemoved = true;
        origLines += c;
      }
      i++;
    }

    origLine += origLines;
    modLine += modLines;

    chunks.push({
      kind: hasAdded && hasRemoved ? 'modify' : hasAdded ? 'insert' : 'delete',
      origStart,
      origEnd: origStart + origLines,
      modStart,
      modEnd: modStart + modLines
    });
  }

  return chunks;
};

export const buildVisualRows = (
  chunks: DiffChunk[],
  side: DiffSide,
  totalLines: number,
  context: number,
  collapse: boolean
): VisualRow[] => {
  if (totalLines <= 0) return [];

  if (!collapse) {
    return Array.from({ length: totalLines }, (_, i) => ({ kind: 'line', lineNumber: i + 1 }) as VisualRow);
  }

  const visible = new Set<number>();
  const expose = (from: number, to: number) => {
    const a = Math.max(1, from);
    const b = Math.min(totalLines, to);
    for (let l = a; l <= b; l++) visible.add(l);
  };

  for (const c of chunks) {
    const start = side === 'orig' ? c.origStart : c.modStart;
    const end = side === 'orig' ? c.origEnd : c.modEnd;
    if (end > start) expose(start - context, end - 1 + context);
    else expose(start - context, start + context - 1);
  }

  const rows: VisualRow[] = [];
  let skipStart = 0;
  let skipLen = 0;
  let skipIndex = 0;

  const flush = () => {
    if (skipLen === 0) return;
    rows.push({
      kind: 'skip',
      count: skipLen,
      firstHidden: skipStart,
      lastHidden: skipStart + skipLen - 1,
      index: skipIndex++
    });
    skipLen = 0;
  };

  for (let l = 1; l <= totalLines; l++) {
    if (visible.has(l)) {
      flush();
      rows.push({ kind: 'line', lineNumber: l });
      continue;
    }
    if (skipLen === 0) skipStart = l;
    skipLen++;
  }
  flush();

  return rows;
};

export const expandRows = (rows: VisualRow[], expanded: ReadonlySet<number>): VisualRow[] => {
  if (expanded.size === 0) return rows;

  return rows.flatMap((row) => {
    if (row.kind !== 'skip' || !expanded.has(row.index)) return [row];
    return Array.from({ length: row.count }, (_, i) => ({ kind: 'line', lineNumber: row.firstHidden + i }) as VisualRow);
  });
};

export const buildUnifiedRows = (oldText: string, newText: string, chunks: DiffChunk[]): UnifiedRow[] => {
  const totalOld = oldText.split('\n').length;
  const totalNew = newText.split('\n').length;

  const rows: UnifiedRow[] = [];
  let oi = 1;
  let ni = 1;

  for (const chunk of chunks) {
    while (oi < chunk.origStart && ni < chunk.modStart) {
      rows.push({ kind: 'unchanged', oldLine: oi, newLine: ni });
      oi++;
      ni++;
    }
    for (let l = chunk.origStart; l < chunk.origEnd; l++) rows.push({ kind: 'deleted', oldLine: l });
    for (let l = chunk.modStart; l < chunk.modEnd; l++) rows.push({ kind: 'added', newLine: l });
    oi = chunk.origEnd;
    ni = chunk.modEnd;
  }

  while (oi <= totalOld && ni <= totalNew) {
    rows.push({ kind: 'unchanged', oldLine: oi, newLine: ni });
    oi++;
    ni++;
  }

  return rows;
};

export const collapseUnified = (rows: UnifiedRow[], context: number): UnifiedVisual[] => {
  const keep = new Set<number>();
  rows.forEach((row, i) => {
    if (row.kind === 'unchanged') return;
    const from = Math.max(0, i - context);
    const to = Math.min(rows.length - 1, i + context);
    for (let j = from; j <= to; j++) keep.add(j);
  });

  const out: UnifiedVisual[] = [];
  let hidden: UnifiedRow[] = [];
  let index = 0;

  const flush = () => {
    if (hidden.length === 0) return;
    out.push({ kind: 'skip', rows: hidden, index: index++ });
    hidden = [];
  };

  rows.forEach((row, i) => {
    if (keep.has(i)) {
      flush();
      out.push({ kind: 'row', row });
      return;
    }
    hidden.push(row);
  });
  flush();

  return out;
};

export const revertChunk = (oldText: string, newText: string, chunk: DiffChunk): string => {
  const oldLines = oldText.split('\n');
  const newLines = newText.split('\n');
  const restored = oldLines.slice(chunk.origStart - 1, chunk.origEnd - 1);
  newLines.splice(chunk.modStart - 1, chunk.modEnd - chunk.modStart, ...restored);
  return newLines.join('\n');
};

const lineSimilarity = (a: string, b: string): number => {
  if (a === b) return 1;
  const ta = a.match(/\w+/g) ?? [];
  const tb = b.match(/\w+/g) ?? [];
  if (ta.length === 0 && tb.length === 0) return 1;
  if (ta.length === 0 || tb.length === 0) return 0;

  const counts = new Map<string, number>();
  for (const t of ta) counts.set(t, (counts.get(t) ?? 0) + 1);

  let common = 0;
  for (const t of tb) {
    const c = counts.get(t) ?? 0;
    if (c > 0) {
      common++;
      counts.set(t, c - 1);
    }
  }

  return (2 * common) / (ta.length + tb.length);
};

const alignChunkLines = (oldChunk: string[], newChunk: string[]): Array<{ oldIdx: number; newIdx: number }> => {
  const n = oldChunk.length;
  const m = newChunk.length;
  if (n === 0 || m === 0) return [];

  const dp: Float32Array[] = Array.from({ length: n + 1 }, () => new Float32Array(m + 1));
  const trace: Uint8Array[] = Array.from({ length: n + 1 }, () => new Uint8Array(m + 1));

  for (let i = 1; i <= n; i++) {
    for (let j = 1; j <= m; j++) {
      const sim = lineSimilarity(oldChunk[i - 1], newChunk[j - 1]);
      const canMatch = sim >= ALIGN_MIN_SIMILARITY;
      const matchScore = canMatch ? dp[i - 1][j - 1] + sim : -Infinity;
      const skipOld = dp[i - 1][j];
      const skipNew = dp[i][j - 1];

      if (canMatch && matchScore >= skipOld && matchScore >= skipNew) {
        dp[i][j] = matchScore;
        trace[i][j] = 0;
      } else if (skipOld >= skipNew) {
        dp[i][j] = skipOld;
        trace[i][j] = 1;
      } else {
        dp[i][j] = skipNew;
        trace[i][j] = 2;
      }
    }
  }

  const pairs: Array<{ oldIdx: number; newIdx: number }> = [];
  let i = n;
  let j = m;

  while (i > 0 && j > 0) {
    const t = trace[i][j];
    if (t === 0) {
      pairs.push({ oldIdx: i - 1, newIdx: j - 1 });
      i--;
      j--;
    } else if (t === 1) {
      i--;
    } else {
      j--;
    }
  }

  return pairs.reverse();
};

const isSpace = (cc: number): boolean => cc === 0x20 || cc === 0x09 || cc === 0x0a || cc === 0x0d;

const isWord = (ch: string): boolean => /\w/.test(ch);

interface WordToken {
  text: string;
  start: number;
  end: number;
}

const wordTokens = (s: string): WordToken[] => {
  const out: WordToken[] = [];
  const re = /\w+/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(s))) out.push({ text: m[0], start: m.index, end: m.index + m[0].length });
  return out;
};

export const coalesceRanges = (raw: string, ranges: IntraLineRange[]): IntraLineRange[] => {
  const merged: IntraLineRange[] = [];

  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && last.kind === r.kind && /^\s*$/.test(raw.slice(last.endCol, r.startCol))) last.endCol = r.endCol;
    else merged.push({ ...r });
  }

  const out: IntraLineRange[] = [];

  for (const r of merged) {
    let a = r.startCol;
    let b = r.endCol;
    while (a < b && isSpace(raw.charCodeAt(a))) a++;
    while (b > a && isSpace(raw.charCodeAt(b - 1))) b--;
    if (b > a) out.push({ startCol: a, endCol: b, kind: r.kind });
  }

  return out;
};

const push = (map: Map<number, IntraLineRange[]>, line: number, range: IntraLineRange) => {
  const arr = map.get(line) ?? [];
  arr.push(range);
  map.set(line, arr);
};

const diffLinePair = (
  oldRaw: string,
  newRaw: string,
  oldLine: number,
  newLine: number,
  mode: 'words' | 'characters',
  oldByLine: Map<number, IntraLineRange[]>,
  newByLine: Map<number, IntraLineRange[]>
) => {
  let oldRanges: IntraLineRange[] = [];
  let newRanges: IntraLineRange[] = [];

  if (mode === 'characters') {
    let oCol = 0;
    let nCol = 0;

    for (const p of diffChars(oldRaw, newRaw)) {
      const len = p.value.length;
      if (p.added) {
        if (len > 0) newRanges.push({ startCol: nCol, endCol: nCol + len, kind: 'insert' });
        nCol += len;
      } else if (p.removed) {
        if (len > 0) oldRanges.push({ startCol: oCol, endCol: oCol + len, kind: 'delete' });
        oCol += len;
      } else {
        oCol += len;
        nCol += len;
      }
    }
  } else {
    const oldToks = wordTokens(oldRaw);
    const newToks = wordTokens(newRaw);
    const parts = diffArrays(
      oldToks.map((t) => t.text),
      newToks.map((t) => t.text)
    );

    const emit = (oldA: number, oldB: number, newA: number, newB: number) => {
      let a1 = oldA;
      let b1 = oldB;
      let a2 = newA;
      let b2 = newB;
      while (a1 < b1 && a2 < b2 && oldRaw[a1] === newRaw[a2]) {
        a1++;
        a2++;
      }
      while (b1 > a1 && b2 > a2 && oldRaw[b1 - 1] === newRaw[b2 - 1]) {
        b1--;
        b2--;
      }
      while (a1 < b1 && isSpace(oldRaw.charCodeAt(a1))) a1++;
      while (b1 > a1 && isSpace(oldRaw.charCodeAt(b1 - 1))) b1--;
      while (a2 < b2 && isSpace(newRaw.charCodeAt(a2))) a2++;
      while (b2 > a2 && isSpace(newRaw.charCodeAt(b2 - 1))) b2--;
      const kind: ChunkKind = b1 > a1 && b2 > a2 ? 'modify' : b1 > a1 ? 'delete' : 'insert';
      if (b1 > a1) {
        while (a1 > 0 && isWord(oldRaw[a1]) && isWord(oldRaw[a1 - 1])) a1--;
        while (b1 < oldRaw.length && isWord(oldRaw[b1 - 1]) && isWord(oldRaw[b1])) b1++;
        oldRanges.push({ startCol: a1, endCol: b1, kind });
      }
      if (b2 > a2) {
        while (a2 > 0 && isWord(newRaw[a2]) && isWord(newRaw[a2 - 1])) a2--;
        while (b2 < newRaw.length && isWord(newRaw[b2 - 1]) && isWord(newRaw[b2])) b2++;
        newRanges.push({ startCol: a2, endCol: b2, kind });
      }
    };

    let oi = 0;
    let ni = 0;
    let oldFrom = 0;
    let newFrom = 0;

    for (const p of parts) {
      const n = p.value.length;
      if (p.added) ni += n;
      else if (p.removed) oi += n;
      else {
        for (let k = 0; k < n; k++) {
          const ot = oldToks[oi + k];
          const nt = newToks[ni + k];
          emit(oldFrom, ot.start, newFrom, nt.start);
          oldFrom = ot.end;
          newFrom = nt.end;
        }
        oi += n;
        ni += n;
      }
    }

    emit(oldFrom, oldRaw.length, newFrom, newRaw.length);
    oldRanges = coalesceRanges(oldRaw, oldRanges);
    newRanges = coalesceRanges(newRaw, newRanges);
  }

  for (const r of oldRanges) push(oldByLine, oldLine, r);
  for (const r of newRanges) push(newByLine, newLine, r);
};

export const computeIntraLine = (
  oldText: string,
  newText: string,
  chunks: DiffChunk[],
  mode: HighlightMode
): IntraLineHighlights => {
  const oldByLine = new Map<number, IntraLineRange[]>();
  const newByLine = new Map<number, IntraLineRange[]>();
  const oldKindOverride = new Map<number, ChunkKind>();
  const newKindOverride = new Map<number, ChunkKind>();
  const result = { oldByLine, newByLine, oldKindOverride, newKindOverride };

  if (mode !== 'words' && mode !== 'characters') return result;

  const oldLines = oldText.split('\n');
  const newLines = newText.split('\n');

  for (const chunk of chunks) {
    if (chunk.kind !== 'modify') continue;

    const oldCount = chunk.origEnd - chunk.origStart;
    const newCount = chunk.modEnd - chunk.modStart;
    if (oldCount === 0 || newCount === 0) continue;
    if (oldCount * newCount > ALIGN_MAX_CELLS) continue;

    const oldChunk = oldLines.slice(chunk.origStart - 1, chunk.origEnd - 1);
    const newChunk = newLines.slice(chunk.modStart - 1, chunk.modEnd - 1);
    const pairs = alignChunkLines(oldChunk, newChunk);
    const oldMatched = new Uint8Array(oldCount);
    const newMatched = new Uint8Array(newCount);

    for (const { oldIdx, newIdx } of pairs) {
      oldMatched[oldIdx] = 1;
      newMatched[newIdx] = 1;
      diffLinePair(
        oldChunk[oldIdx],
        newChunk[newIdx],
        chunk.origStart + oldIdx,
        chunk.modStart + newIdx,
        mode,
        oldByLine,
        newByLine
      );
    }

    for (let k = 0; k < oldCount; k++) if (!oldMatched[k]) oldKindOverride.set(chunk.origStart + k, 'delete');
    for (let k = 0; k < newCount; k++) if (!newMatched[k]) newKindOverride.set(chunk.modStart + k, 'insert');
  }

  return result;
};

export const mapByAnchors = (anchors: ScrollAnchor[], sourceY: number): number => {
  if (anchors.length === 0) return sourceY;
  if (sourceY <= anchors[0].sourceY) return anchors[0].targetY + (sourceY - anchors[0].sourceY);

  for (let i = 0; i < anchors.length - 1; i++) {
    const a = anchors[i];
    const b = anchors[i + 1];
    if (sourceY < a.sourceY || sourceY > b.sourceY) continue;
    const span = b.sourceY - a.sourceY;
    if (span <= 0) return a.targetY;
    return a.targetY + (b.targetY - a.targetY) * ((sourceY - a.sourceY) / span);
  }

  const last = anchors[anchors.length - 1];
  return last.targetY + (sourceY - last.sourceY);
};

export const buildAnchors = (chunks: DiffChunk[], from: PaneApi, to: PaneApi, side: DiffSide): ScrollAnchor[] => {
  const out: ScrollAnchor[] = [{ sourceY: 0, targetY: 0 }];

  for (const c of chunks) {
    const sLine = side === 'orig' ? c.origStart : c.modStart;
    const tLine = side === 'orig' ? c.modStart : c.origStart;
    out.push({
      sourceY: from.getTopForLine(Math.max(1, sLine)),
      targetY: to.getTopForLine(Math.max(1, tLine))
    });
  }

  return out;
};

export const firstAfter = (topAt: (index: number) => number, count: number, y: number): number => {
  let lo = 0;
  let hi = count - 1;

  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (topAt(mid) < y) lo = mid + 1;
    else hi = mid;
  }

  return Math.max(0, lo - 1);
};

export const lineKinds =(chunks: DiffChunk[], side: DiffSide): Map<number, ChunkKind> => {
  const map = new Map<number, ChunkKind>();

  for (const c of chunks) {
    const start = side === 'orig' ? c.origStart : c.modStart;
    const end = side === 'orig' ? c.origEnd : c.modEnd;
    for (let l = start; l < end; l++) map.set(l, c.kind);
  }

  return map;
};
