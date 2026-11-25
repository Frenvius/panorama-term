export interface ShowAgentRequest {
  tileId: string;
  cwd: string;
}

const AGENT_SHOW_EVENT = 'agent:show';

export const requestShowAgent = (req: ShowAgentRequest): void => {
  window.dispatchEvent(new CustomEvent(AGENT_SHOW_EVENT, { detail: req }));
};

export const onShowAgentRequest = (cb: (req: ShowAgentRequest) => void): (() => void) => {
  const handler = (e: Event) => cb((e as CustomEvent<ShowAgentRequest>).detail);
  window.addEventListener(AGENT_SHOW_EVENT, handler);
  return () => window.removeEventListener(AGENT_SHOW_EVENT, handler);
};
