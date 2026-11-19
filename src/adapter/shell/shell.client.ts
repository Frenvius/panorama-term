import { invoke } from '@tauri-apps/api/core';
import { isRemote } from '~/adapter/transport';

export const revealPath = (path: string): void => {
  if (isRemote()) return;
  void invoke('reveal_path', { path }).catch(() => {});
};

export const openUrl = (url: string): void => {
  void invoke('open_url', { url }).catch(() => {});
};

export interface IdeInfo {
  id: string;
  label: string;
  command: string;
  family: string;
  recommended: boolean;
}

let ideCache: IdeInfo[] = [];

export const cachedIdes = (): IdeInfo[] => (isRemote() ? [] : ideCache);

export const detectIdes = async (path: string): Promise<IdeInfo[]> => {
  if (isRemote()) return [];
  const list = await invoke<IdeInfo[]>('detect_ides', { path }).catch(() => []);
  if (list.length) ideCache = list;
  return list;
};

export const openInIde = (path: string, command: string, family: string): void => {
  if (isRemote()) return;
  void invoke('open_in_ide', { path, command, family }).catch(() => {});
};
