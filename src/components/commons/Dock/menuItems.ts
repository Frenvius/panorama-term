import type { DockLayout, DockMode, DockSide, PanelId } from '~/domain/interfaces/dock.interface';
import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import { MODE_PANELS } from '~/usecase/util/dockLayout';
import { PANEL_DESCRIPTORS } from './panels';

export const buildIconMenu = (
  id: PanelId,
  currentSide: DockSide,
  move: (id: PanelId, side: DockSide) => void,
  hide: (id: PanelId) => void
): ContextMenuEntry[] => [
  { label: 'Dock Left', checked: currentSide === 'left', onSelect: () => move(id, 'left') },
  { label: 'Dock Right', checked: currentSide === 'right', onSelect: () => move(id, 'right') },
  'separator',
  { label: 'Hide Button', onSelect: () => hide(id) }
];

interface StripMenuOptions {
  mode: DockMode;
  layout: DockLayout;
  reset: () => void;
  show: (id: PanelId) => void;
  hide: (id: PanelId) => void;
}

export const buildStripMenu = ({ mode, layout, show, hide, reset }: StripMenuOptions): ContextMenuEntry[] => [
  ...MODE_PANELS[mode].map((id): ContextMenuEntry => {
    const visible = !layout.hidden.includes(id);
    const toggle = (): void => (visible ? hide(id) : show(id));
    return { label: PANEL_DESCRIPTORS[id].label, checked: visible, onSelect: toggle };
  }),
  'separator',
  { label: 'Reset Layout', onSelect: reset }
];
