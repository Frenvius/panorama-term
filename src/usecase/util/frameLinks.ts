import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';

import { frameOf } from '~/usecase/util/frame';

export type FrameLink = { kind: 'note'; a: string; b: string } | { kind: 'term'; a: string; b: string };

const isEligibleTerm = (t: Tile): boolean =>
  t.type === 'term' && !t.runCwd && !t.runId && !t.orchWorker;

export const frameLinks = (tiles: Tile[], frames: Frame[]): FrameLink[] => {
  const byFrame = new Map<string, { notes: Tile[]; terms: Tile[] }>();

  for (const tile of tiles) {
    if (tile.type !== 'note' && !isEligibleTerm(tile)) continue;
    const frame = frameOf(frames, tile);
    if (!frame) continue;
    let group = byFrame.get(frame.id);
    if (!group) {
      group = { notes: [], terms: [] };
      byFrame.set(frame.id, group);
    }
    if (tile.type === 'note') group.notes.push(tile);
    else group.terms.push(tile);
  }

  const links: FrameLink[] = [];
  for (const { notes, terms } of byFrame.values()) {
    for (const note of notes) {
      for (const term of terms) {
        links.push({ kind: 'note', a: note.id, b: term.id });
      }
    }
    for (let i = 0; i < terms.length; i++) {
      for (let j = i + 1; j < terms.length; j++) {
        const [a, b] = terms[i].id < terms[j].id
          ? [terms[i].id, terms[j].id]
          : [terms[j].id, terms[i].id];
        links.push({ kind: 'term', a, b });
      }
    }
  }
  return links;
};

export const isManualLink = (tiles: Tile[], link: FrameLink): boolean => {
  if (link.kind === 'note') {
    const note = tiles.find((t) => t.id === link.a);
    return (note?.linkedTo ?? []).includes(link.b);
  }
  const ta = tiles.find((t) => t.id === link.a);
  const tb = tiles.find((t) => t.id === link.b);
  return (ta?.linkedTo ?? []).includes(link.b) || (tb?.linkedTo ?? []).includes(link.a);
};

export const inSameFrame = (frames: Frame[], a: Tile | undefined, b: Tile | undefined): boolean => {
  const fa = a ? frameOf(frames, a) : null;
  const fb = b ? frameOf(frames, b) : null;
  return Boolean(fa && fb && fa.id === fb.id);
};

export const linkKey =(link: FrameLink): string => `${link.kind}:${link.a}:${link.b}`;
