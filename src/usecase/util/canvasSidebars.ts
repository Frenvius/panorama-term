import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type CanvasSidebarMode = 'overlay' | 'docked';

export const CANVAS_SIDEBAR_MODE_KEY = 'canvasSidebarMode';
export const CANVAS_SIDEBAR_MODE_EVENT = 'panorama-canvas-sidebar-mode';

export const getCanvasSidebarMode = (): CanvasSidebarMode => getSetting<CanvasSidebarMode>(CANVAS_SIDEBAR_MODE_KEY, 'overlay');

export const setCanvasSidebarMode = (mode: CanvasSidebarMode): void => {
  void setSetting(CANVAS_SIDEBAR_MODE_KEY, mode);
  window.dispatchEvent(new Event(CANVAS_SIDEBAR_MODE_EVENT));
};
