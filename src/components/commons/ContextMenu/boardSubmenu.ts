import type { RunSummary } from '~/domain/interfaces/orch.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

export const boardSubmenu = (
  runs: RunSummary[],
  onOpen: (runId: string) => void,
  onNew: () => void
): ContextMenuEntry[] => [
  ...runs.map((s) => ({ label: s.run.title, onSelect: () => onOpen(s.run.id) })),
  ...(runs.length > 0 ? (['separator'] as ContextMenuEntry[]) : []),
  { label: 'New board...', onSelect: onNew }
];
