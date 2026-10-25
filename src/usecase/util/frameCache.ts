import type { GridFrame } from '~/domain/interfaces/pty.interface';

const MAX_FRAMES = 64;

const frames = new Map<string, GridFrame>();

export const rememberFrame = (sessionKey: string, frame: GridFrame): void => {
  frames.delete(sessionKey);
  frames.set(sessionKey, frame);
  if (frames.size <= MAX_FRAMES) return;
  const oldest = frames.keys().next().value;
  if (oldest !== undefined) frames.delete(oldest);
};

export const recallFrame = (sessionKey: string): GridFrame | null => frames.get(sessionKey) ?? null;

export const forgetFrame = (sessionKey: string): void => {
  frames.delete(sessionKey);
};
