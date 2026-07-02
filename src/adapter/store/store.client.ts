import { invoke } from '@tauri-apps/api/core';

export const storeList = (dir: string): Promise<string[]> => invoke('store_list', { dir });

export const storeDelete = (name: string): Promise<void> => invoke('store_delete', { name });

export const storeWrite = (name: string, value: unknown): Promise<void> => invoke('store_write', { name, value });

export const storeRead = <T>(name: string): Promise<T | null> => invoke('store_read', { name });
