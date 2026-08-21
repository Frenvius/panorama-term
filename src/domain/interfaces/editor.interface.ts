export type EditorTabKind = 'file' | 'diff';

export interface EditorTab {
  key: string;
  kind: EditorTabKind;
  root: string;
  path: string;
  commit?: string;
  preview?: boolean;
}
