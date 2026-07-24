import { invoke } from '@tauri-apps/api/core';

export const revealPath = (path: string): void => {
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

export const cachedIdes = (): IdeInfo[] => ideCache;

export const detectIdes = async (path: string): Promise<IdeInfo[]> => {
  const list = await invoke<IdeInfo[]>('detect_ides', { path }).catch(() => []);
  if (list.length) ideCache = list;
  return list;
};

export const openInIde = (path: string, command: string, family: string): void => {
  void invoke('open_in_ide', { path, command, family }).catch(() => {});
};
