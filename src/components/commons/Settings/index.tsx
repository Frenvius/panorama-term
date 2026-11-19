import type { LucideIcon } from 'lucide-react';

import React from 'react';
import { X, Bot, Bell, Search, Puzzle, Palette, Workflow, Keyboard, Monitor, Sparkles, Smartphone, LayoutGrid, SquareTerminal } from 'lucide-react';

import { SETTINGS_SECTIONS, searchSettings, type SettingsSection } from '~/usecase/util/settingsNav';
import { resetCanvas, resetAppearance, resetTerminal, resetNotifications, resetAgents, resetOrchestration, resetAiActions, resetShortcuts, RESET_DESCRIPTIONS } from '~/usecase/util/settingsDefaults';
import Button from './Button';
import ConfirmDialog from './ConfirmDialog';
import CanvasSection from './CanvasSection';
import AgentsSection from './AgentsSection';
import RemoteSection from './RemoteSection';
import PluginsSection from './PluginsSection';
import DevicesSection from './DevicesSection';
import TerminalSection from './TerminalSection';
import ShortcutsSection from './ShortcutsSection';
import AppearanceSection from './AppearanceSection';
import AiActionsSection from './AiActionsSection';
import OrchestrationSection from './OrchestrationSection';
import NotificationsSection from './NotificationsSection';
import styles from './styles.module.scss';

interface SettingsProps {
  onClose: () => void;
  initialSection?: SettingsSection;
}

const SECTION_ICONS: Record<SettingsSection, LucideIcon> = {
  canvas: LayoutGrid,
  appearance: Palette,
  terminal: SquareTerminal,
  notifications: Bell,
  agents: Bot,
  orchestration: Workflow,
  aiActions: Sparkles,
  devices: Smartphone,
  remote: Monitor,
  plugins: Puzzle,
  shortcuts: Keyboard
};

interface SectionMeta {
  title: string;
  subtitle?: string;
  hasReset?: boolean;
}

const SECTION_META: Record<SettingsSection, SectionMeta> = {
  canvas: { title: 'Canvas', subtitle: 'Canvas behavior and navigation aids.', hasReset: true },
  appearance: { title: 'Appearance', subtitle: 'Theme applies across the whole app.', hasReset: true },
  terminal: { title: 'Terminal', subtitle: 'Changes apply to new terminals.', hasReset: true },
  notifications: { title: 'Notifications', subtitle: 'Toasts, their position and monitor.', hasReset: true },
  agents: { title: 'Agents', subtitle: 'Model picker and Claude Code settings.', hasReset: true },
  orchestration: { title: 'Orchestration', subtitle: 'Agents and models used for multi-agent runs.', hasReset: true },
  aiActions: { title: 'AI actions', subtitle: 'Provider and model for AI-powered actions.', hasReset: true },
  devices: { title: 'Devices', subtitle: 'Manage phone and desktop connections.' },
  remote: { title: 'Remote host', subtitle: 'Use the workspace and terminals of a paired desktop.' },
  plugins: { title: 'Plugins', subtitle: 'Extend Panorama with themes, fonts and more.' },
  shortcuts: { title: 'Shortcuts', subtitle: 'Click a shortcut and press the keys to rebind it.', hasReset: true }
};

const GROUP_LABELS: Record<string, string> = {
  WORKSPACE: 'Workspace',
  AGENTS: 'Agents',
  CONNECTIONS: 'Connections',
  SYSTEM: 'System'
};

const SECTION_RESET_FNS: Partial<Record<SettingsSection, () => void>> = {
  canvas: resetCanvas,
  appearance: resetAppearance,
  terminal: resetTerminal,
  notifications: resetNotifications,
  agents: resetAgents,
  orchestration: resetOrchestration,
  aiActions: resetAiActions,
  shortcuts: resetShortcuts
};

const GROUPS = [...new Set(SETTINGS_SECTIONS.map((s) => s.group))];

const Settings = ({ initialSection = 'canvas', onClose }: SettingsProps) => {
  const [section, setSection] = React.useState<SettingsSection>(initialSection);
  const [query, setQuery] = React.useState('');
  const [searchCursor, setSearchCursor] = React.useState(0);
  const [confirmReset, setConfirmReset] = React.useState<SettingsSection | null>(null);
  const [resetKeys, setResetKeys] = React.useState<Partial<Record<SettingsSection, number>>>({});
  const panelRef = React.useRef<HTMLDivElement>(null);
  const bodyRef = React.useRef<HTMLDivElement>(null);
  const searchRef = React.useRef<HTMLInputElement>(null);

  const searchResults = React.useMemo(() => searchSettings(query), [query]);

  const hasInnerLayer = () => !!document.querySelector('[data-settings-layer]');

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      if (hasInnerLayer()) return;
      onClose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  const pickSection = (id: SettingsSection) => {
    setSection(id);
    setQuery('');
  };

  const pickResult = (entry: { id: string; section: SettingsSection; label: string }) => {
    setSection(entry.section);
    setQuery('');
    requestAnimationFrame(() => {
      const body = bodyRef.current;
      if (!body) return;
      const el = body.querySelector<HTMLElement>(`[data-setting="${entry.id}"]`);
      if (!el) return;
      const elRect = el.getBoundingClientRect();
      const bodyRect = body.getBoundingClientRect();
      const relTop = elRect.top - bodyRect.top + body.scrollTop;
      body.scrollTop = relTop - body.clientHeight / 2 + elRect.height / 2;
      el.dataset.flash = '';
      setTimeout(() => delete el.dataset.flash, 1200);
    });
  };

  const handleSearchKey = (e: React.KeyboardEvent) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setSearchCursor((c) => Math.min(c + 1, searchResults.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setSearchCursor((c) => Math.max(c - 1, 0));
    } else if (e.key === 'Enter' && searchResults.length > 0) {
      e.preventDefault();
      pickResult(searchResults[searchCursor]);
    }
  };

  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setSearchCursor(0);
  };

  const openRestore = () => setConfirmReset(section);
  const closeRestore = () => setConfirmReset(null);

  const doRestore = () => {
    if (!confirmReset) return;
    SECTION_RESET_FNS[confirmReset]?.();
    setResetKeys((keys) => ({ ...keys, [confirmReset]: (keys[confirmReset] ?? 0) + 1 }));
    setConfirmReset(null);
  };

  const meta = SECTION_META[section];
  const resetKey = resetKeys[section] ?? 0;

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div ref={panelRef} className={styles.panel} onMouseDown={(e) => e.stopPropagation()}>
        {confirmReset && (
          <ConfirmDialog
            title={`Restore ${SECTION_META[confirmReset].title} defaults?`}
            message={RESET_DESCRIPTIONS[confirmReset] ?? 'All settings for this section will be reset.'}
            confirmLabel="Restore"
            onConfirm={doRestore}
            onCancel={closeRestore}
          />
        )}
        <button className={styles.close} onClick={onClose} aria-label="Close settings">
          <X size={15} strokeWidth={1.75} />
        </button>
        <aside className={styles.sidebar}>
          <h1 className={styles.sidebarTitle}>Settings</h1>
          <div className={styles.searchBox}>
            <Search size={14} strokeWidth={1.75} className={styles.searchIcon} />
            <input
              ref={searchRef}
              type="text"
              value={query}
              onChange={handleQueryChange}
              onKeyDown={handleSearchKey}
              placeholder="Search settings"
              className={styles.searchInput}
              aria-label="Search settings"
            />
          </div>
          {query.trim() ? (
            <div className={styles.searchResults}>
              {searchResults.length === 0 ? (
                <span className={styles.searchEmpty}>No settings match.</span>
              ) : (
                searchResults.map((result, i) => {
                  const sectionLabel = SETTINGS_SECTIONS.find((s) => s.id === result.section)?.label ?? '';
                  const handleClick = () => pickResult(result);
                  return (
                    <button
                      key={result.id}
                      onClick={handleClick}
                      className={`${styles.searchResult} ${i === searchCursor ? styles.searchResultActive : ''}`}
                    >
                      <span className={styles.searchResultLabel}>{result.label}</span>
                      <span className={styles.searchResultSection}>{sectionLabel}</span>
                    </button>
                  );
                })
              )}
            </div>
          ) : (
            GROUPS.map((group) => (
              <div key={group} className={styles.navGroup}>
                <span className={styles.navGroupLabel}>{GROUP_LABELS[group]}</span>
                {SETTINGS_SECTIONS.filter((s) => s.group === group).map(({ id, label }) => {
                  const Icon = SECTION_ICONS[id];
                  const handleClick = () => pickSection(id);
                  return (
                    <button
                      key={id}
                      type="button"
                      onClick={handleClick}
                      className={`${styles.navItem} ${section === id ? styles.navActive : ''}`}
                    >
                      <Icon size={15} strokeWidth={1.75} />
                      <span>{label}</span>
                    </button>
                  );
                })}
              </div>
            ))
          )}
        </aside>
        <div className={styles.content}>
          <div className={styles.sectionHeader}>
            <div className={styles.sectionMeta}>
              <h2 className={styles.sectionTitle}>{meta.title}</h2>
              {meta.subtitle && <p className={styles.sectionSubtitle}>{meta.subtitle}</p>}
            </div>
            {meta.hasReset && (
              <Button variant="ghost" size="sm" onClick={openRestore}>Restore defaults</Button>
            )}
          </div>
          <div ref={bodyRef} className={styles.sectionBody}>
            {section === 'canvas' && <CanvasSection key={resetKey} />}
            {section === 'appearance' && <AppearanceSection key={resetKey} />}
            {section === 'terminal' && <TerminalSection key={resetKey} />}
            {section === 'notifications' && <NotificationsSection key={resetKey} />}
            {section === 'agents' && <AgentsSection key={resetKey} />}
            {section === 'orchestration' && <OrchestrationSection key={resetKey} />}
            {section === 'aiActions' && <AiActionsSection key={resetKey} />}
            {section === 'devices' && <DevicesSection />}
            {section === 'remote' && <RemoteSection />}
            {section === 'plugins' && <PluginsSection />}
            {section === 'shortcuts' && <ShortcutsSection key={resetKey} />}
          </div>
        </div>
      </div>
    </div>
  );
};

export default Settings;
