import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type HeaderPart =
  | 'icon'
  | 'folder'
  | 'branch'
  | 'diff'
  | 'build'
  | 'run'
  | 'link'
  | 'fullscreen'
  | 'restart';

export type HeaderParts = Record<HeaderPart, boolean>;

export const HEADER_PARTS_KEY = 'headerParts';
export const HEADER_PARTS_EVENT = 'panorama-header-parts';

export const HEADER_PART_OPTIONS: { id: HeaderPart; label: string; description: string }[] = [
  { id: 'icon', label: 'Agent icon', description: 'Logo of the agent detected in the terminal.' },
  { id: 'folder', label: 'Folder name', description: 'Working directory chip next to the title.' },
  { id: 'branch', label: 'Git branch', description: 'Branch name with ahead/behind markers.' },
  { id: 'diff', label: 'Diff stats', description: 'Lines added and removed by the agent.' },
  { id: 'build', label: 'Build button', description: 'Hammer button and its command menu.' },
  { id: 'run', label: 'Run button', description: 'Play button and its command menu.' },
  { id: 'link', label: 'Link button', description: 'Drag handle to link tiles together.' },
  { id: 'fullscreen', label: 'Fullscreen button', description: 'Expand the tile to the whole canvas.' },
  { id: 'restart', label: 'Restart button', description: 'Restart the shell running in the tile.' }
];

const DEFAULTS = Object.fromEntries(HEADER_PART_OPTIONS.map((o) => [o.id, true])) as HeaderParts;

export const getHeaderParts = (): HeaderParts => ({
  ...DEFAULTS,
  ...getSetting<Partial<HeaderParts>>(HEADER_PARTS_KEY, {})
});

export const setHeaderPart = (id: HeaderPart, on: boolean): void => {
  void setSetting(HEADER_PARTS_KEY, { ...getHeaderParts(), [id]: on });
  window.dispatchEvent(new Event(HEADER_PARTS_EVENT));
};
