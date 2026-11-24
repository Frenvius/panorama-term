import type { FrameLink } from '~/usecase/util/frameLinks';
import type { Tile, Frame } from '~/domain/interfaces/canvas.interface';

import React from 'react';

import { termName, noteLinkTitle } from '~/usecase/util/noteLink';
import { frameLinks, isManualLink, linkKey } from '~/usecase/util/frameLinks';
import { linkNote, unlinkNote, linkTerm, unlinkTerm } from '~/adapter/notes/notes.client';

interface SyncedEntry {
  link: FrameLink;
  aKey: string;
  bKey: string;
}

const entryKey = (entry: SyncedEntry): string => `${linkKey(entry.link)}:${entry.aKey}:${entry.bKey}`;

interface UseFrameLinksArgs {
  scope: string;
  wsId: string | null;
  tiles: Tile[];
  frames: Frame[];
  paused: boolean;
}

export const useFrameLinks = ({ scope, wsId, tiles, frames, paused }: UseFrameLinksArgs): void => {
  const syncedRef = React.useRef<Map<string, SyncedEntry>>(new Map());
  const prevScopeRef = React.useRef<string>(scope);

  React.useEffect(() => {
    if (scope !== prevScopeRef.current) {
      prevScopeRef.current = scope;
      syncedRef.current = new Map();
    }
  }, [scope]);

  React.useEffect(() => {
    if (paused || !wsId) return;
    const id = setTimeout(() => {
      const desired = frameLinks(tiles, frames);
      const desiredEntries: SyncedEntry[] = [];
      for (const link of desired) {
        if (link.kind === 'note') {
          const note = tiles.find((t) => t.id === link.a);
          const term = tiles.find((t) => t.id === link.b);
          if (!note || !term) continue;
          desiredEntries.push({ link, aKey: link.a, bKey: term.ptySessionId ?? term.id });
        } else {
          const ta = tiles.find((t) => t.id === link.a);
          const tb = tiles.find((t) => t.id === link.b);
          if (!ta || !tb) continue;
          desiredEntries.push({ link, aKey: ta.ptySessionId ?? ta.id, bKey: tb.ptySessionId ?? tb.id });
        }
      }

      const desiredByKey = new Map<string, SyncedEntry>(desiredEntries.map((e) => [entryKey(e), e]));
      const current = syncedRef.current;
      const next = new Map<string, SyncedEntry>();

      for (const entry of desiredEntries) {
        const key = entryKey(entry);
        if (current.has(key)) {
          next.set(key, current.get(key)!);
        } else {
          next.set(key, entry);
          if (entry.link.kind === 'note') {
            const note = tiles.find((t) => t.id === entry.link.a)!;
            void linkNote(wsId, entry.link.a, entry.bKey, noteLinkTitle(note)).catch(() => {});
          } else {
            const ta = tiles.find((t) => t.id === entry.link.a)!;
            const tb = tiles.find((t) => t.id === entry.link.b)!;
            void linkTerm(entry.aKey, termName(ta), entry.bKey, termName(tb)).catch(() => {});
          }
        }
      }

      for (const [key, entry] of current) {
        if (desiredByKey.has(key)) continue;
        if (isManualLink(tiles, entry.link)) continue;
        if (entry.link.kind === 'note') {
          void unlinkNote(entry.aKey, entry.bKey).catch(() => {});
        } else {
          void unlinkTerm(entry.aKey, entry.bKey).catch(() => {});
        }
      }

      syncedRef.current = next;
    }, 400);

    return () => clearTimeout(id);
  }, [scope, wsId, tiles, frames, paused]);
};
