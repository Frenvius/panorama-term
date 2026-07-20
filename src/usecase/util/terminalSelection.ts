export interface Cell {
  row: number;
  col: number;
}

export interface Selection {
  a: Cell;
  b: Cell;
}

export const WORD_SEPARATORS = ' ()[]{}\',"`';

export const wordSelection = (line: string, row: number, col: number): Selection => {
  const chars = Array.from(line);
  const sep = (i: number): boolean => {
    const ch = chars[i];
    return ch === undefined || WORD_SEPARATORS.includes(ch);
  };
  const target = sep(col);
  let c0 = col;
  let c1 = col;
  while (c0 > 0 && sep(c0 - 1) === target) c0--;
  while (c1 < chars.length - 1 && sep(c1 + 1) === target) c1++;
  return { a: { row, col: c0 }, b: { row, col: c1 } };
};

export interface UrlSegment {
  row: number;
  c0: number;
  c1: number;
}

export interface UrlSpan {
  url: string;
  segments: UrlSegment[];
}

const isUrlChar = (c: string | undefined): boolean => c !== undefined && c !== ' ' && c !== '\t';

export const urlSpanAt = (lines: string[], cols: number, row: number, col: number): UrlSpan | null => {
  const rowChars = (r: number): string[] => {
    const c = Array.from(lines[r] ?? '');
    while (c.length < cols) c.push(' ');
    return c;
  };
  const wrapped = (r: number): boolean => isUrlChar(rowChars(r)[cols - 1]);

  let top = row;
  while (top > 0 && wrapped(top - 1)) top--;
  let bot = row;
  while (bot < lines.length - 1 && wrapped(bot)) bot++;

  const block: string[] = [];
  for (let r = top; r <= bot; r++) block.push(...rowChars(r));

  const g = (row - top) * cols + col;
  if (!isUrlChar(block[g])) return null;
  let s = g;
  let e = g;
  while (s > 0 && isUrlChar(block[s - 1])) s--;
  while (e < block.length - 1 && isUrlChar(block[e + 1])) e++;

  const m = block.slice(s, e + 1).join('').match(/https?:\/\/\S+/i);
  if (!m || m.index === undefined) return null;
  const url = m[0].replace(/[.,;:!?)\]}'"]+$/, '');
  const g0 = s + m.index;
  const g1 = g0 + Array.from(url).length - 1;

  const segments: UrlSegment[] = [];
  for (let gi = g0; gi <= g1; ) {
    const r = top + Math.floor(gi / cols);
    const c0 = gi % cols;
    const c1 = Math.min(cols - 1, c0 + (g1 - gi));
    segments.push({ row: r, c0, c1 });
    gi += c1 - c0 + 1;
  }
  return { url, segments };
};

export const lineSelection = (row: number, cols: number): Selection => ({
  a: { row, col: 0 },
  b: { row, col: cols - 1 }
});

export const orderSel = (s: Selection): { s: Cell; e: Cell } => {
  const { a, b } = s;
  if (a.row < b.row || (a.row === b.row && a.col <= b.col)) return { s: a, e: b };
  return { s: b, e: a };
};

export const selectText = (lines: string[], cols: number, sel: Selection): string => {
  const { s, e } = orderSel(sel);
  const out: string[] = [];
  for (let r = s.row; r <= e.row; r++) {
    const chars = Array.from(lines[r] ?? '');
    const c0 = r === s.row ? s.col : 0;
    const c1 = r === e.row ? e.col : cols - 1;
    let line = '';
    for (let c = c0; c <= c1; c++) line += chars[c] ?? ' ';
    out.push(line.replace(/\s+$/, ''));
  }
  return out.join('\n');
};
