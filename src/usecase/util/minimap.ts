import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export const MINIMAP_PINNED_KEY = 'minimapPinned';
export const MINIMAP_PINNED_EVENT = 'panorama-minimap-pinned';

export const getMinimapPinned = (): boolean => getSetting(MINIMAP_PINNED_KEY, false);

export const setMinimapPinned = (on: boolean): void => {
  void setSetting(MINIMAP_PINNED_KEY, on);
  window.dispatchEvent(new Event(MINIMAP_PINNED_EVENT));
};
