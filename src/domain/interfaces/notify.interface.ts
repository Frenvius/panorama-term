export type NotifyKind = 'finished' | 'attention' | 'permission' | 'idle' | 'generic';

export interface NotifyPayload {
  id: number;
  tileId: string;
  wsId: string | null;
  tabId: string | null;
  kind: NotifyKind;
  title: string;
  text?: string;
}

export interface NotifyTarget {
  tileId: string;
  wsId: string | null;
  tabId: string | null;
}
