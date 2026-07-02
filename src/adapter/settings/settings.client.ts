import { storeRead, storeWrite } from '~/adapter/store/store.client';

export type Settings = Record<string, unknown>;

const FILE = 'config.json';

export const loadSettings = async (): Promise<Settings> => (await storeRead<Settings>(FILE)) ?? {};

export const saveSettings = (settings: Settings): Promise<void> => storeWrite(FILE, settings);
