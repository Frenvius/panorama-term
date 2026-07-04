export const writeClipboard = (text: string): void => {
  navigator.clipboard?.writeText(text).catch(() => {});
};

export const readClipboard = (): Promise<string> => {
  if (!navigator.clipboard?.readText) return Promise.resolve('');
  return navigator.clipboard.readText().catch(() => '');
};
