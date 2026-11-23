import type { RunSummary } from '~/domain/interfaces/orch.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import { normalizeRepo } from '~/usecase/util/orch';

export const boardSubmenu = (
  runs: RunSummary[],
  onOpen: (runId: string) => void,
  onNew: () => void,
  repo?: string
): ContextMenuEntry[] => [
  ...runs.map((s) => ({ label: repo && normalizeRepo(repo) === normalizeRepo(s.run.repo) ? s.run.title : `${s.run.title} (${s.run.repo.replace(/[\\/]+$/, '').split(/[\\/]/).pop()})`, onSelect: () => onOpen(s.run.id) })),
  ...(runs.length > 0 ? (['separator'] as ContextMenuEntry[]) : []),
  { label: 'New board...', onSelect: onNew }
];
