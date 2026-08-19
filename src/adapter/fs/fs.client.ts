import { invoke } from '@tauri-apps/api/core';

export interface DirEntry {
  name: string;
  path: string;
  dir: boolean;
}

export const readDir = (path: string): Promise<DirEntry[]> =>
  invoke<DirEntry[]>('read_dir', { path }).catch(() => [] as DirEntry[]);

export const readTextFile = (path: string): Promise<string> => invoke<string>('read_text_file', { path });

export const writeTextFile = (path: string, content: string): Promise<void> =>
  invoke<void>('write_text_file', { path, content });

export const watchFile = (path: string): Promise<number> => invoke<number>('watch_text_file', { path });

export const unwatchFile = (id: number): Promise<void> => invoke<void>('unwatch_text_file', { id });
