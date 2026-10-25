const PROMPT_RESTORE_EVENT = 'panorama-prompt-restore';

interface PromptRestoreDetail {
  text: string;
  tileId: string;
}

export const emitPromptRestore = (tileId: string, text: string): void => {
  window.dispatchEvent(new CustomEvent<PromptRestoreDetail>(PROMPT_RESTORE_EVENT, { detail: { tileId, text } }));
};

export const subscribePromptRestore = (tileId: string, handler: (text: string) => void): (() => void) => {
  const listener = (e: Event) => {
    const detail = (e as CustomEvent<PromptRestoreDetail>).detail;
    if (detail.tileId === tileId) handler(detail.text);
  };
  window.addEventListener(PROMPT_RESTORE_EVENT, listener);
  return () => window.removeEventListener(PROMPT_RESTORE_EVENT, listener);
};
