import { storeRead, storeWrite } from '~/adapter/store/store.client';

const FILE = 'ui-state.json';
const SAVE_DELAY = 300;
const PREFIXES = ['panorama:', 'agent:'];

let cache: Record<string, string> = {};
let timer: ReturnType<typeof setTimeout> | undefined;

const flush = (): void => {
  timer = undefined;
  void storeWrite(FILE, cache);
};

const schedule = (): void => {
  if (timer) return;
  timer = setTimeout(flush, SAVE_DELAY);
};

const legacyKeys = (): string[] => {
  const keys: string[] = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i);
    if (key && PREFIXES.some((prefix) => key.startsWith(prefix))) keys.push(key);
  }
  return keys;
};

const importLegacy = async (): Promise<void> => {
  const keys = legacyKeys();
  if (keys.length === 0) return;
  let added = 0;
  for (const key of keys) {
    const value = localStorage.getItem(key);
    if (value !== null && !(key in cache)) {
      cache[key] = value;
      added++;
    }
  }
  if (added > 0) await storeWrite(FILE, cache);
  for (const key of keys) localStorage.removeItem(key);
};

export const initPrefs = async (): Promise<void> => {
  cache = (await storeRead<Record<string, string>>(FILE).catch(() => null)) ?? {};
  await importLegacy().catch(() => {});
};

export const getPref = (key: string): string | null => cache[key] ?? null;

export const setPref = (key: string, value: string): void => {
  if (cache[key] === value) return;
  cache[key] = value;
  schedule();
};

export const removePref = (key: string): void => {
  if (!(key in cache)) return;
  delete cache[key];
  schedule();
};
