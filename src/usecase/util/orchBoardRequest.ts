export interface OrchBoardRequest {
  runId: string;
  nearTileId: string;
}

const projectLastBoard = new Map<string, string>();

export const setProjectLastBoard = (cwd: string, runId: string): void => {
  if (cwd) projectLastBoard.set(cwd, runId);
};

export const getProjectLastBoard = (cwd: string): string | undefined => projectLastBoard.get(cwd);

export interface ShowWorkerRequest {
  tileId: string;
  cwd: string;
  group: { id: string; title: string };
}

const BOARD_EVENT = 'orch:board:request';
const WORKER_EVENT = 'orch:worker:show';

export const requestOrchBoard = (req: OrchBoardRequest): void => {
  window.dispatchEvent(new CustomEvent(BOARD_EVENT, { detail: req }));
};

export const onOrchBoardRequest = (cb: (req: OrchBoardRequest) => void): (() => void) => {
  const handler = (e: Event) => cb((e as CustomEvent<OrchBoardRequest>).detail);
  window.addEventListener(BOARD_EVENT, handler);
  return () => window.removeEventListener(BOARD_EVENT, handler);
};

export const requestShowWorker = (req: ShowWorkerRequest): void => {
  window.dispatchEvent(new CustomEvent(WORKER_EVENT, { detail: req }));
};

export const onShowWorkerRequest = (cb: (req: ShowWorkerRequest) => void): (() => void) => {
  const handler = (e: Event) => cb((e as CustomEvent<ShowWorkerRequest>).detail);
  window.addEventListener(WORKER_EVENT, handler);
  return () => window.removeEventListener(WORKER_EVENT, handler);
};

export interface OrchDiffRequest {
  root: string;
  file: string;
  commit?: string;
  base?: string;
}

const DIFF_EVENT = 'orch:board:diff';

export const requestOrchDiff = (req: OrchDiffRequest): void => {
  window.dispatchEvent(new CustomEvent(DIFF_EVENT, { detail: req }));
};

export const onOrchDiffRequest = (cb: (req: OrchDiffRequest) => void): (() => void) => {
  const handler = (e: Event) => cb((e as CustomEvent<OrchDiffRequest>).detail);
  window.addEventListener(DIFF_EVENT, handler);
  return () => window.removeEventListener(DIFF_EVENT, handler);
};
