export interface OpenDiffOpts {
  commit?: string;
  base?: string;
}

export interface ShowWorkerOpts {
  tileId: string;
  cwd: string;
  group: { id: string; title: string };
}

export interface OrchPanelProps {
  onOpenDiff: (root: string, file: string, opts: OpenDiffOpts) => void;
  onShowWorker: (opts: ShowWorkerOpts) => void;
}
