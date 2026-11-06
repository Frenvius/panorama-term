export type SettingsSection = 'general' | 'appearance' | 'terminal' | 'agentBar' | 'mobile' | 'shortcuts';

export const SETTINGS_SECTIONS: { id: SettingsSection; label: string }[] = [
  { id: 'general', label: 'General' },
  { id: 'appearance', label: 'Appearance' },
  { id: 'terminal', label: 'Terminal' },
  { id: 'agentBar', label: 'Agent bar' },
  { id: 'mobile', label: 'Mobile' },
  { id: 'shortcuts', label: 'Shortcuts' }
];

const EVENT = 'settings:open';

export const requestSettings = (section: SettingsSection): void => {
  window.dispatchEvent(new CustomEvent<SettingsSection>(EVENT, { detail: section }));
};

export const onSettingsRequest = (handler: (section: SettingsSection) => void): (() => void) => {
  const listener = (e: Event) => handler((e as CustomEvent<SettingsSection>).detail);
  window.addEventListener(EVENT, listener);
  return () => window.removeEventListener(EVENT, listener);
};
