import { KEYBINDINGS } from '~/usecase/util/keybindings';

export type SettingsSection =
  | 'canvas'
  | 'appearance'
  | 'terminal'
  | 'notifications'
  | 'agents'
  | 'orchestration'
  | 'aiActions'
  | 'devices'
  | 'remote'
  | 'plugins'
  | 'shortcuts';

export type SettingsGroup = 'WORKSPACE' | 'AGENTS' | 'CONNECTIONS' | 'SYSTEM';

export const SETTINGS_SECTIONS: { id: SettingsSection; label: string; group: SettingsGroup }[] = [
  { id: 'canvas', label: 'Canvas', group: 'WORKSPACE' },
  { id: 'appearance', label: 'Appearance', group: 'WORKSPACE' },
  { id: 'terminal', label: 'Terminal', group: 'WORKSPACE' },
  { id: 'notifications', label: 'Notifications', group: 'WORKSPACE' },
  { id: 'agents', label: 'Agents', group: 'AGENTS' },
  { id: 'orchestration', label: 'Orchestration', group: 'AGENTS' },
  { id: 'aiActions', label: 'AI actions', group: 'AGENTS' },
  { id: 'devices', label: 'Devices', group: 'CONNECTIONS' },
  { id: 'remote', label: 'Remote host', group: 'CONNECTIONS' },
  { id: 'plugins', label: 'Plugins', group: 'SYSTEM' },
  { id: 'shortcuts', label: 'Shortcuts', group: 'SYSTEM' }
];

export interface SettingsIndexEntry {
  id: string;
  section: SettingsSection;
  label: string;
  keywords?: string[];
}

export const SETTINGS_INDEX: SettingsIndexEntry[] = [
  { id: 'canvas.maxZoom', section: 'canvas', label: 'Maximum zoom', keywords: ['zoom', 'scale'] },
  { id: 'canvas.reorderMode', section: 'canvas', label: 'Reorder layout', keywords: ['layout', 'spaced', 'compact'] },
  { id: 'canvas.gap', section: 'canvas', label: 'Gap', keywords: ['padding', 'space', 'tiles', 'frames'] },
  { id: 'canvas.minimapPinned', section: 'canvas', label: 'Always visible minimap', keywords: ['minimap', 'pinned'] },
  { id: 'canvas.minimapCorner', section: 'canvas', label: 'Minimap position', keywords: ['minimap', 'corner'] },
  { id: 'canvas.sidebarMode', section: 'canvas', label: 'Sidebar placement', keywords: ['sidebar', 'float', 'beside', 'docked'] },
  { id: 'appearance.theme', section: 'appearance', label: 'Theme mode', keywords: ['theme', 'dark', 'light', 'system'] },
  { id: 'appearance.darkTheme', section: 'appearance', label: 'Dark theme', keywords: ['dark theme'] },
  { id: 'appearance.lightTheme', section: 'appearance', label: 'Light theme', keywords: ['light theme'] },
  { id: 'appearance.interfaceFont', section: 'appearance', label: 'Interface font', keywords: ['font', 'ui font'] },
  { id: 'appearance.codeFont', section: 'appearance', label: 'Code font', keywords: ['font', 'code', 'mono'] },
  { id: 'appearance.tileHeader', section: 'appearance', label: 'Tile header parts', keywords: ['tile', 'header'] },
  { id: 'terminal.shell', section: 'terminal', label: 'Default shell', keywords: ['shell', 'bash', 'powershell'] },
  { id: 'terminal.font', section: 'terminal', label: 'Terminal font', keywords: ['font', 'terminal', 'hack'] },
  { id: 'notifications.toasts', section: 'notifications', label: 'Show toasts', keywords: ['notification', 'toast', 'popup', 'disable', 'mute'] },
  { id: 'notifications.corner', section: 'notifications', label: 'Toast position', keywords: ['notification', 'toast', 'corner'] },
  { id: 'notifications.display', section: 'notifications', label: 'Toast display', keywords: ['notification', 'monitor', 'display'] },
  { id: 'agents.modelPicker', section: 'agents', label: 'Model providers', keywords: ['provider', 'model'] },
  { id: 'agents.usage', section: 'agents', label: 'Status bar usage', keywords: ['usage', 'limit', 'quota', 'status bar'] },
  { id: 'agents.idleCompaction', section: 'agents', label: 'Idle compaction', keywords: ['compact', 'idle', 'claude'] },
  { id: 'orchestration.workerTiles', section: 'orchestration', label: 'Open worker tiles automatically', keywords: ['worker', 'tiles'] },
  { id: 'orchestration.hint', section: 'orchestration', label: 'Orchestrate hint', keywords: ['hint', 'orchestration', 'auto', 'suggest'] },
  { id: 'orchestration.roles', section: 'orchestration', label: 'Role chains', keywords: ['role', 'chain', 'model'] },
  { id: 'orchestration.providers', section: 'orchestration', label: 'Providers', keywords: ['provider', 'api', 'custom'] },
  { id: 'aiActions.default', section: 'aiActions', label: 'Default provider', keywords: ['ai', 'provider', 'model', 'default'] },
  { id: 'aiActions.commitMessage', section: 'aiActions', label: 'Commit message', keywords: ['ai', 'commit', 'message'] },
  { id: 'aiActions.tidy', section: 'aiActions', label: 'Tidy projects', keywords: ['ai', 'tidy', 'projects'] },
  { id: 'plugins.installed', section: 'plugins', label: 'Installed plugins', keywords: ['plugin', 'extension'] },
  { id: 'plugins.folder', section: 'plugins', label: 'Plugins folder', keywords: ['plugin', 'folder', 'directory'] },
  ...KEYBINDINGS.map((kb) => ({
    id: `shortcuts.${kb.id}`,
    section: 'shortcuts' as SettingsSection,
    label: kb.label,
    keywords: [kb.group, kb.defaultCombo]
  }))
];

export const searchSettings = (query: string): SettingsIndexEntry[] => {
  if (!query.trim()) return [];
  const q = query.toLowerCase();
  return SETTINGS_INDEX.filter((entry) => {
    const sectionLabel = SETTINGS_SECTIONS.find((s) => s.id === entry.section)?.label ?? '';
    return (
      entry.label.toLowerCase().includes(q) ||
      sectionLabel.toLowerCase().includes(q) ||
      entry.keywords?.some((k) => k.toLowerCase().includes(q))
    );
  }).slice(0, 12);
};

const EVENT = 'settings:open';

export const requestSettings = (section: SettingsSection): void => {
  window.dispatchEvent(new CustomEvent<SettingsSection>(EVENT, { detail: section }));
};

export const onSettingsRequest = (handler: (section: SettingsSection) => void): (() => void) => {
  const listener = (e: Event) => handler((e as CustomEvent<SettingsSection>).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
};
