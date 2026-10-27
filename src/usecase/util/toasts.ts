export type ToastKind = 'info' | 'error' | 'success' | 'progress';

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface ToastInput {
  kind: ToastKind;
  detail?: string;
  message: string;
  action?: ToastAction;
}

export interface Toast extends ToastInput {
  id: number;
}

export interface ToastTask<T> {
  failed: string;
  pending: string;
  done: (result: T) => ToastInput | null;
  failure?: (err: unknown) => ToastInput;
}

const MAX_TOASTS = 3;
const ACTION_MS = 8000;
const PENDING_DELAY_MS = 400;
const DISMISS_MS: Record<ToastKind, number | null> = { info: 4000, error: null, success: 4000, progress: null };

const listeners = new Set<() => void>();
const timers = new Map<number, number>();

let toasts: Toast[] = [];
let nextId = 1;

const commit = (next: Toast[]): void => {
  toasts = next;
  listeners.forEach((fn) => fn());
};

const schedule = (toast: Toast): void => {
  window.clearTimeout(timers.get(toast.id));
  timers.delete(toast.id);
  const ms = toast.kind === 'success' && toast.action ? ACTION_MS : DISMISS_MS[toast.kind];
  if (ms !== null) timers.set(toast.id, window.setTimeout(() => dismissToast(toast.id), ms));
};

export const getToasts = (): Toast[] => toasts;

export const subscribeToasts = (fn: () => void): (() => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

export const dismissToast = (id: number): void => {
  window.clearTimeout(timers.get(id));
  timers.delete(id);
  if (toasts.some((t) => t.id === id)) commit(toasts.filter((t) => t.id !== id));
};

export const showToast = (input: ToastInput): number => {
  const toast = { ...input, id: nextId++ };
  const kept = toasts.slice(-(MAX_TOASTS - 1));
  toasts.filter((t) => !kept.includes(t)).forEach((t) => window.clearTimeout(timers.get(t.id)));
  commit([...kept, toast]);
  schedule(toast);
  return toast.id;
};

export const updateToast = (id: number, input: ToastInput): void => {
  if (!toasts.some((t) => t.id === id)) {
    showToast(input);
    return;
  }
  const toast = { ...input, id };
  commit(toasts.map((t) => (t.id === id ? toast : t)));
  schedule(toast);
};

export const errorText = (err: unknown): string => (typeof err === 'string' ? err : String(err));

export const toastTask = <T>(task: Promise<T>, { pending, failed, done, failure }: ToastTask<T>): Promise<T> => {
  let id: number | null = null;
  const timer = window.setTimeout(() => {
    id = showToast({ kind: 'progress', message: pending });
  }, PENDING_DELAY_MS);

  const settle = (input: ToastInput | null) => {
    window.clearTimeout(timer);
    if (!input) {
      if (id !== null) dismissToast(id);
      return;
    }
    if (id === null) showToast(input);
    else updateToast(id, input);
  };

  return task.then(
    (result) => {
      settle(done(result));
      return result;
    },
    (err: unknown) => {
      settle(failure ? failure(err) : { kind: 'error', message: failed, detail: errorText(err) });
      throw err;
    }
  );
};
