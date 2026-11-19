import type { Layout, LayoutSnapshot } from '~/usecase/util/layoutHistory';
import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

import React from 'react';

import { captureLayout, layoutChanged, restoreLayout } from '~/usecase/util/layoutHistory';

interface UseLayoutHistoryArgs {
  tiles: Tile[];
  frames: Frame[];
  scope: string;
  interacting: boolean;
  apply: (layout: Layout) => void;
}

const SETTLE_MS = 250;
const MAX_ENTRIES = 100;

const isEditable = (el: EventTarget | null): boolean =>
  el instanceof HTMLElement && (el.isContentEditable || el.matches('input, textarea, select'));

const isHistoryKey = (e: KeyboardEvent): boolean => (e.ctrlKey || e.metaKey) && /^[zy]$/i.test(e.key);

export const useLayoutHistory = ({ tiles, frames, scope, interacting, apply }: UseLayoutHistoryArgs) => {
  const layoutRef = React.useRef<Layout>({ tiles, frames });
  layoutRef.current = { tiles, frames };
  const interactingRef = React.useRef(interacting);
  interactingRef.current = interacting;
  const applyRef = React.useRef(apply);
  applyRef.current = apply;

  const baseline = React.useRef<LayoutSnapshot>(captureLayout({ tiles, frames }));
  const undoStack = React.useRef<LayoutSnapshot[]>([]);
  const redoStack = React.useRef<LayoutSnapshot[]>([]);
  const changedAt = React.useRef(0);
  const editedAt = React.useRef(0);

  const commit = React.useCallback(() => {
    const prev = baseline.current;
    const current = captureLayout(layoutRef.current);
    baseline.current = current;
    if (!layoutChanged(prev, current)) return;
    undoStack.current = [...undoStack.current, prev].slice(-MAX_ENTRIES);
    redoStack.current = [];
    changedAt.current = performance.now();
  }, []);

  React.useEffect(() => {
    if (interacting) return;
    const id = setTimeout(commit, SETTLE_MS);
    return () => clearTimeout(id);
  }, [tiles, frames, interacting, commit]);

  React.useEffect(() => {
    undoStack.current = [];
    redoStack.current = [];
    baseline.current = captureLayout(layoutRef.current);
  }, [scope]);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (isEditable(e.target) && !isHistoryKey(e) && !['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) {
        editedAt.current = performance.now();
      }
    };
    const onFocus = (e: FocusEvent) => {
      if (isEditable(e.target)) editedAt.current = performance.now();
    };
    window.addEventListener('keydown', onKey, true);
    window.addEventListener('focusin', onFocus, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      window.removeEventListener('focusin', onFocus, true);
    };
  }, []);

  const rebase = React.useCallback((layout: Layout) => {
    baseline.current = captureLayout(layout);
  }, []);

  const ownsShortcut = () => !isEditable(document.activeElement) || changedAt.current > editedAt.current;

  const step = (from: React.RefObject<LayoutSnapshot[]>, to: React.RefObject<LayoutSnapshot[]>): boolean => {
    if (interactingRef.current || !ownsShortcut()) return false;
    commit();
    const current = layoutRef.current;
    while (from.current.length) {
      const next = restoreLayout(from.current.pop()!, current);
      if (!next) continue;
      to.current.push(captureLayout(current));
      baseline.current = captureLayout(next);
      changedAt.current = performance.now();
      applyRef.current(next);
      return true;
    }
    return false;
  };

  const undo = React.useCallback(() => step(undoStack, redoStack), []);
  const redo = React.useCallback(() => step(redoStack, undoStack), []);

  return { undo, redo, rebase };
};
