import { setSetting } from '~/adapter/settings/settings.client';
import { setTabScope } from '~/usecase/util/tabScope';
import { KEYBINDINGS, resetBinding } from '~/usecase/util/keybindings';
import { DEFAULT_FONT, setFontChoice } from '~/usecase/util/fonts';
import { setTasksOpen, setTasksLink, setTasksAuto, setTasksDetail } from '~/usecase/util/bgTasks';
import { setNotifToasts, setNotifPlacement, setNotifHideOverFullscreen } from '~/usecase/util/notifPlacement';
import { TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
import { setMinimapCorner, setMinimapPinned } from '~/usecase/util/minimap';
import { setCanvasSidebarMode } from '~/usecase/util/canvasSidebars';
import { HEADER_PARTS_KEY, HEADER_PARTS_EVENT } from '~/usecase/util/headerParts';
import { AGENT_PROVIDERS_KEY, getSeenProviders } from '~/usecase/util/agentProviders';
import { BUILTIN_THEME, setThemeChoice, setThemePref } from '~/usecase/util/theme';
import { orchSettingsActions } from '~/usecase/hooks/useOrchSettings';
import { setUsageProviders } from '~/usecase/hooks/useUsageProviders';
import { MAX_ZOOM_KEY, FRAME_PAD_KEY, FRAME_PAD_DEFAULT, REORDER_MODE_KEY, USAGE_PROVIDERS, IDLE_COMPACTION_KEY } from '~/usecase/util/constants';

export const resetCanvas = (): void => {
  setTabScope('project');
  void setSetting(MAX_ZOOM_KEY, 1);
  void setSetting(REORDER_MODE_KEY, 'default');
  void setSetting(FRAME_PAD_KEY, FRAME_PAD_DEFAULT);
  setMinimapPinned(false);
  setMinimapCorner('bottom-right');
  setCanvasSidebarMode('overlay');
};

export const resetAppearance = (): void => {
  setThemePref('system');
  setThemeChoice('dark', BUILTIN_THEME);
  setThemeChoice('light', BUILTIN_THEME);
  void setFontChoice('ui', DEFAULT_FONT);
  void setFontChoice('code', DEFAULT_FONT);
  void setSetting(HEADER_PARTS_KEY, {});
  window.dispatchEvent(new Event(HEADER_PARTS_EVENT));
};

export const resetTerminal = (): void => {
  void setSetting(TERMINAL_TARGET_KEY, 'auto');
  void setFontChoice('terminal', DEFAULT_FONT);
};

export const resetNotifications = (): void => {
  setNotifToasts(true);
  setNotifHideOverFullscreen(true);
  setNotifPlacement({ corner: 'bottom-right', monitor: null });
};

export const resetAgents = (): void => {
  void setSetting(AGENT_PROVIDERS_KEY, { seen: getSeenProviders(), hidden: [] });
  void setSetting(IDLE_COMPACTION_KEY, true);
  setTasksOpen('inline');
  setTasksLink('dashed');
  setTasksAuto(false);
  setTasksDetail('normal');
  setUsageProviders(USAGE_PROVIDERS);
};

export const resetOrchestration = (): void => {
  void orchSettingsActions.updateSettings({ showWorkerTiles: false, orchestrateHint: 'suggest' });
};

export const resetAiActions = (): void => {
  void orchSettingsActions.updateSettings({ aiActions: { default: { provider: 'claude', model: 'haiku' }, overrides: {} } });
};

export const resetShortcuts = (): void => {
  for (const kb of KEYBINDINGS) void resetBinding(kb.id);
};

export const RESET_DESCRIPTIONS: Partial<Record<string, string>> = {
  canvas: 'Tab scope, maximum zoom, layout mode, gap, minimap visibility and corner, sidebar placement.',
  appearance: 'Theme mode, dark and light theme, interface and code fonts, tile header parts.',
  terminal: 'Default shell and terminal font.',
  notifications: 'Toasts on, hidden over fullscreen apps, toast corner and display monitor.',
  agents: 'Model provider visibility list, idle compaction and background tasks.',
  orchestration: 'Worker tile visibility and orchestrate hint. Role chains and providers are not changed.',
  aiActions: 'Default provider and model, and all action overrides.',
  shortcuts: 'All keyboard shortcuts back to their defaults.'
};
