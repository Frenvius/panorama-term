export interface OpenTerminalRequest {
  tileId: string;
  cwd?: string;
  command: string;
}

const EVENT = 'terminal:open';

export const createTileId = (): string => Math.random().toString(36).slice(2, 10);

export const requestOpenTerminal = (req: OpenTerminalRequest): void => {
  window.dispatchEvent(new CustomEvent<OpenTerminalRequest>(EVENT, { detail: req }));
};

export const onOpenTerminalRequest = (cb: (req: OpenTerminalRequest) => void): (() => void) => {
  const handler = (e: Event) => cb((e as CustomEvent<OpenTerminalRequest>).detail);
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
};
