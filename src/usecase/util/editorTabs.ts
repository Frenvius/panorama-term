import type { EditorTab, EditorTabKind } from '~/domain/interfaces/editor.interface';

const DIFF_KEY = 'diff';

interface OpenRequest {
  kind: EditorTabKind;
  root: string;
  path: string;
  preview?: boolean;
}

export const tabKey = (kind: EditorTabKind, path: string): string => (kind === 'diff' ? DIFF_KEY : `file:${path}`);

export const openTab = (tabs: EditorTab[], request: OpenRequest): EditorTab[] => {
  const { kind, root, path, preview } = request;
  const key = tabKey(kind, path);
  const next: EditorTab = { key, kind, root, path, preview: preview || undefined };

  if (tabs.some((t) => t.key === key)) {
    return tabs.map((t) => (t.key === key ? { ...next, preview: preview ? t.preview : undefined } : t));
  }

  const slot = preview ? tabs.findIndex((t) => t.preview) : -1;
  return slot >= 0 ? tabs.map((t, i) => (i === slot ? next : t)) : [...tabs, next];
};

export const pinTab = (tabs: EditorTab[], key: string): EditorTab[] =>
  tabs.map((t) => (t.key === key ? { ...t, preview: undefined } : t));
