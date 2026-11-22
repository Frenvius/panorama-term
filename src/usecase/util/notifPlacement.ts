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
export const NOTIF_FULLSCREEN_EVENT = 'notif:hide-over-fullscreen';

const CORNER_KEY = 'notifCorner';
const MONITOR_KEY = 'notifMonitor';
const TOASTS_KEY = 'notifToasts';
const FULLSCREEN_KEY = 'notifHideOverFullscreen';

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

export const getNotifToasts = (): boolean => getSetting(TOASTS_KEY, true);

export const setNotifToasts = (on: boolean): void => void setSetting(TOASTS_KEY, on);

export const getNotifHideOverFullscreen = (): boolean => getSetting(FULLSCREEN_KEY, true);

export const setNotifHideOverFullscreen = (on: boolean): void => {
  void setSetting(FULLSCREEN_KEY, on);
  void emit(NOTIF_FULLSCREEN_EVENT, on);
};

export const isOtherAppFullscreen = (monitor: string | null): Promise<boolean> =>
  invoke<boolean>('notif_fullscreen', { monitor });

export const listMonitors = (): Promise<MonitorInfo[]> => invoke<MonitorInfo[]>('list_monitors');

export const monitorLabel = (name: string): string => name.replace(/^\\\\[.?]\\/, '') || 'Display';
