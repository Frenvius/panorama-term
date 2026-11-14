export interface StoreChange {
  name: string;
  revision: number;
  origin: string | null;
}

export const shouldReload = (event: StoreChange, origin: string): boolean =>
  event.origin !== origin && (event.name === 'workspaces.json' || /^workspaces\/[^/]+\.json$/.test(event.name));

export const createDeferredReload = (reload: (names: string[]) => void) => {
  let down = false;
  const pending = new Set<string>();
  const flush = () => {
    if (down || !pending.size) return;
    const names = [...pending];
    pending.clear();
    reload(names);
  };
  return {
    start: () => {
      down = true;
    },
    end: () => {
      down = false;
      flush();
    },
    change: (name: string) => {
      pending.add(name);
      flush();
    }
  };
};

export const shouldPersist = (
  applied: { tiles: unknown; frames: unknown },
  current: { tiles: unknown; frames: unknown },
  remote = true
): boolean => !remote || applied.tiles !== current.tiles || applied.frames !== current.frames;

export const listenPointerInteraction = (target: Window, start: (event: PointerEvent) => void, end: () => void): (() => void) => {
  target.addEventListener('pointerdown', start, true);
  target.addEventListener('pointerup', end, true);
  target.addEventListener('pointercancel', end, true);
  target.addEventListener('blur', end);
  return () => {
    target.removeEventListener('pointerdown', start, true);
    target.removeEventListener('pointerup', end, true);
    target.removeEventListener('pointercancel', end, true);
    target.removeEventListener('blur', end);
  };
};
