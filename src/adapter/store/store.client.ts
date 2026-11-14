import { invoke } from '~/adapter/transport';

export const storeOrigin = crypto.randomUUID();

export const storeList = (dir: string): Promise<string[]> => invoke('store_list', { dir });

export const storeDelete = (name: string): Promise<void> => invoke('store_delete', { name, origin: storeOrigin });

export const storeWrite = (name: string, value: unknown): Promise<void> =>
  invoke('store_write', { name, value, origin: storeOrigin });

export const storeRead = <T>(name: string): Promise<T | null> => invoke('store_read', { name });

export const storeWriteMany = (entries: { name: string; value: unknown }[]): Promise<void> =>
  invoke('store_write_many', { entries, origin: storeOrigin });
