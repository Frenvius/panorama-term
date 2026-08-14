import type { NotifCorner } from '~/usecase/util/notifPlacement';
import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type MinimapCorner = NotifCorner;

export const MINIMAP_CORNER_KEY = 'minimapCorner';
export const MINIMAP_PINNED_KEY = 'minimapPinned';
export const MINIMAP_SETTINGS_EVENT = 'panorama-minimap-settings';

export const getMinimapPinned = (): boolean => getSetting(MINIMAP_PINNED_KEY, false);

export const getMinimapCorner = (): MinimapCorner => getSetting<MinimapCorner>(MINIMAP_CORNER_KEY, 'bottom-right');

export const setMinimapPinned = (on: boolean): void => {
  void setSetting(MINIMAP_PINNED_KEY, on);
  window.dispatchEvent(new Event(MINIMAP_SETTINGS_EVENT));
};

export const setMinimapCorner = (corner: MinimapCorner): void => {
  void setSetting(MINIMAP_CORNER_KEY, corner);
  window.dispatchEvent(new Event(MINIMAP_SETTINGS_EVENT));
};
