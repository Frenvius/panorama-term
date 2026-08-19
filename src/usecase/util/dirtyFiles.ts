const dirty = new Set<string>();
const listeners = new Set<() => void>();

export const markDirty = (path: string, value: boolean): void => {
  if (value === dirty.has(path)) return;
  if (value) dirty.add(path);
  else dirty.delete(path);
  listeners.forEach((fn) => fn());
};

export const isDirty = (path: string): boolean => dirty.has(path);

export const subscribeDirty = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export interface EditorRequest {
  handled: boolean;
  path?: string;
  done?: Promise<void>;
}

export const dispatchSave = (path?: string): EditorRequest => {
  const event = new CustomEvent<EditorRequest>('editor:save', { detail: { handled: false, path } });
  window.dispatchEvent(event);
  return event.detail;
};

export const requestSave = async (path?: string): Promise<boolean> => {
  const detail = dispatchSave(path);
  await detail.done;
  return detail.handled;
};

export const requestFind = (): boolean => {
  const event = new CustomEvent<EditorRequest>('editor:find', { detail: { handled: false } });
  window.dispatchEvent(event);
  return event.detail.handled;
};
