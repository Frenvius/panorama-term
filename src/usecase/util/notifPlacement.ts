import { emit } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type NotifCorner = 'top-left' | 'top-right' | 'bottom-left' | 'bottom-right';

export interface NotifPlacement {
  corner: NotifCorner;
  monitor: string | null;
}

export interface MonitorInfo {
  name: string;
  width: number;
  height: number;
  primary: boolean;
}

export const NOTIF_PLACEMENT_EVENT = 'notif:placement';

const CORNER_KEY = 'notifCorner';
const MONITOR_KEY = 'notifMonitor';

export const NOTIF_CORNERS: { id: NotifCorner; label: string }[] = [
  { id: 'top-left', label: 'Top left' },
  { id: 'top-right', label: 'Top right' },
  { id: 'bottom-left', label: 'Bottom left' },
  { id: 'bottom-right', label: 'Bottom right' }
];

export const getNotifPlacement = (): NotifPlacement => ({
  corner: getSetting<NotifCorner>(CORNER_KEY, 'bottom-right'),
  monitor: getSetting<string | null>(MONITOR_KEY, null)
});

export const setNotifPlacement = (placement: NotifPlacement): void => {
  void setSetting(CORNER_KEY, placement.corner);
  void setSetting(MONITOR_KEY, placement.monitor);
  void emit(NOTIF_PLACEMENT_EVENT, placement);
};

export const listMonitors = (): Promise<MonitorInfo[]> => invoke<MonitorInfo[]>('list_monitors');

export const monitorLabel = (name: string): string => name.replace(/^\\\\[.?]\\/, '') || 'Display';
