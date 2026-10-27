export interface RefDiffRequest {
  root: string;
  base: string;
  path: string;
  files: string[];
}

const EVENT = 'editor:ref-diff';

export const requestRefDiff = (request: RefDiffRequest): void => {
  window.dispatchEvent(new CustomEvent<RefDiffRequest>(EVENT, { detail: request }));
};

export const onRefDiff = (handler: (request: RefDiffRequest) => void): (() => void) => {
  const listener = (e: Event) => handler((e as CustomEvent<RefDiffRequest>).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
};
