import { getSetting, setSetting } from '~/adapter/settings/settings.client';

export type CommandId =
  | 'tile.fullscreen'
  | 'tile.new'
  | 'note.new'
  | 'tile.close'
  | 'tile.reopen'
  | 'tile.focus'
  | 'view.resetZoom'
  | 'view.navigator'
  | 'view.palette'
  | 'diff.close'
  | 'diff.prevChunk'
  | 'diff.nextChunk'
  | 'diff.prevFile'
  | 'diff.nextFile';

interface Command {
  id: CommandId;
  label: string;
  group: string;
  defaultCombo: string;
}

const BINDINGS_KEY = 'keybindings';

const IS_MAC = typeof navigator !== 'undefined' && navigator.platform.toLowerCase().includes('mac');

export const KEYBINDINGS: Command[] = [
  { id: 'tile.new', label: 'New terminal', group: 'Canvas', defaultCombo: 'mod+t' },
  { id: 'note.new', label: 'New note', group: 'Canvas', defaultCombo: 'mod+shift+n' },
  { id: 'tile.close', label: 'Close active tile', group: 'Canvas', defaultCombo: 'mod+w' },
  { id: 'tile.reopen', label: 'Reopen closed tile', group: 'Canvas', defaultCombo: 'mod+shift+t' },
  { id: 'tile.fullscreen', label: 'Toggle fullscreen', group: 'Canvas', defaultCombo: 'mod+shift+f' },
  { id: 'tile.focus', label: 'Focus active tile', group: 'Canvas', defaultCombo: 'alt+f' },
  { id: 'view.resetZoom', label: 'Reset zoom', group: 'View', defaultCombo: 'mod+0' },
  { id: 'view.navigator', label: 'Toggle navigator', group: 'View', defaultCombo: 'mod+b' },
  { id: 'view.palette', label: 'Go to tile', group: 'View', defaultCombo: 'shift shift' },
  { id: 'diff.close', label: 'Close viewer', group: 'Diff viewer', defaultCombo: 'escape' },
  { id: 'diff.prevChunk', label: 'Previous difference', group: 'Diff viewer', defaultCombo: 'alt+arrowup' },
  { id: 'diff.nextChunk', label: 'Next difference', group: 'Diff viewer', defaultCombo: 'alt+arrowdown' },
  { id: 'diff.prevFile', label: 'Previous file', group: 'Diff viewer', defaultCombo: 'alt+arrowleft' },
  { id: 'diff.nextFile', label: 'Next file', group: 'Diff viewer', defaultCombo: 'alt+arrowright' }
];

const overrides = (): Record<string, string> => getSetting<Record<string, string>>(BINDINGS_KEY, {});

export const getBinding = (id: CommandId): string => {
  const cmd = KEYBINDINGS.find((c) => c.id === id);
  return overrides()[id] ?? cmd?.defaultCombo ?? '';
};

export const setBinding = (id: CommandId, combo: string): Promise<void> =>
  setSetting(BINDINGS_KEY, { ...overrides(), [id]: combo });

export const resetBinding = (id: CommandId): Promise<void> => {
  const next = { ...overrides() };
  delete next[id];
  return setSetting(BINDINGS_KEY, next);
};

const MODS = new Set(['control', 'shift', 'alt', 'meta']);

const normKey = (e: KeyboardEvent): string => {
  const k = e.key.toLowerCase();
  if (k === ' ') return 'space';
  return k;
};

export const comboFromEvent = (e: KeyboardEvent): string | null => {
  if (MODS.has(e.key.toLowerCase())) return null;
  const parts: string[] = [];
  if (e.ctrlKey || e.metaKey) parts.push('mod');
  if (e.altKey) parts.push('alt');
  if (e.shiftKey) parts.push('shift');
  parts.push(normKey(e));
  return parts.join('+');
};

export const matchCombo = (e: KeyboardEvent, combo: string): boolean => {
  if (!combo) return false;
  const parts = combo.split('+');
  const key = parts[parts.length - 1];
  const want = new Set(parts.slice(0, -1));
  if ((e.ctrlKey || e.metaKey) !== want.has('mod')) return false;
  if (e.shiftKey !== want.has('shift')) return false;
  if (e.altKey !== want.has('alt')) return false;
  return normKey(e) === key;
};

export const matchCommand = (e: KeyboardEvent): CommandId | null => {
  for (const cmd of KEYBINDINGS) {
    if (matchCombo(e, getBinding(cmd.id))) return cmd.id;
  }
  return null;
};

let capturing = false;

export const setCapturing = (value: boolean): void => {
  capturing = value;
};

export const isCapturing = (): boolean => capturing;

const TOKENS: Record<string, string> = {
  'shift shift': 'Double Shift',
  mod: IS_MAC ? 'Cmd' : 'Ctrl',
  alt: IS_MAC ? 'Option' : 'Alt',
  shift: 'Shift',
  space: 'Space',
  escape: 'Esc',
  arrowup: 'Up',
  arrowdown: 'Down',
  arrowleft: 'Left',
  arrowright: 'Right'
};

export const formatCombo = (combo: string): string => {
  if (!combo) return 'Unset';
  return combo
    .split('+')
    .map((p) => TOKENS[p] ?? (p.length === 1 ? p.toUpperCase() : p.charAt(0).toUpperCase() + p.slice(1)))
    .join(' + ');
};
