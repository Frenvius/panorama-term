import React from 'react';
import type { LucideIcon } from 'lucide-react';
import { X, Check, Palette, Keyboard, RotateCcw, SquareTerminal, SlidersHorizontal } from 'lucide-react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { ZOOM_MAX, MAX_ZOOM_KEY, FRAME_PAD_KEY } from '~/usecase/util/constants';
import { getThemePref, setThemePref, type ThemePref } from '~/usecase/util/theme';
import { listTerminalTargets, TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
import { setHeaderPart, getHeaderParts, HEADER_PART_OPTIONS, type HeaderPart } from '~/usecase/util/headerParts';
import {
  getMinimapCorner,
  getMinimapPinned,
  setMinimapCorner,
  setMinimapPinned,
  type MinimapCorner
} from '~/usecase/util/minimap';
import {
  listMonitors,
  monitorLabel,
  NOTIF_CORNERS,
  getNotifPlacement,
  setNotifPlacement,
  type NotifCorner,
  type MonitorInfo
} from '~/usecase/util/notifPlacement';
import {
  KEYBINDINGS,
  getBinding,
  setBinding,
  formatCombo,
  resetBinding,
  setCapturing,
  comboFromEvent,
  type CommandId
} from '~/usecase/util/keybindings';

import styles from './styles.module.scss';

interface SettingsProps {
  onClose: () => void;
}

interface ShortcutRowProps {
  id: CommandId;
  label: string;
  defaultCombo: string;
}

interface ToggleOptionProps {
  label: string;
  checked: boolean;
  description: string;
  onToggle: () => void;
}

interface RadioOptionProps {
  label: string;
  selected: boolean;
  description: string;
  onSelect: () => void;
}

const RadioOption = ({ label, description, selected, onSelect }: RadioOptionProps) => (
  <button
    type="button"
    onClick={onSelect}
    className={`${styles.option} ${selected ? styles.selected : ''}`}
  >
    <span className={styles.radio}>{selected && <span className={styles.dot} />}</span>
    <span className={styles.optionText}>
      <span className={styles.optionLabel}>{label}</span>
      <span className={styles.optionDesc}>{description}</span>
    </span>
  </button>
);

const ToggleOption = ({ label, description, checked, onToggle }: ToggleOptionProps) => (
  <button
    type="button"
    onClick={onToggle}
    className={`${styles.option} ${checked ? styles.selected : ''}`}
  >
    <span className={styles.check}>{checked && <Check size={11} strokeWidth={3} />}</span>
    <span className={styles.optionText}>
      <span className={styles.optionLabel}>{label}</span>
      <span className={styles.optionDesc}>{description}</span>
    </span>
  </button>
);

const ShortcutRow = ({ id, label, defaultCombo }: ShortcutRowProps) => {
  const [combo, setCombo] = React.useState(() => getBinding(id));
  const [listening, setListening] = React.useState(false);

  React.useEffect(() => {
    if (!listening) return;
    setCapturing(true);
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === 'Escape') {
        setListening(false);
        return;
      }
      const next = comboFromEvent(e);
      if (!next) return;
      setCombo(next);
      void setBinding(id, next);
      setListening(false);
    };
    window.addEventListener('keydown', onKey, true);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      setCapturing(false);
    };
  }, [listening, id]);

  const startListening = () => setListening(true);

  const reset = () => {
    void resetBinding(id);
    setCombo(defaultCombo);
  };

  return (
    <div className={styles.shortcut}>
      <span className={styles.shortcutLabel}>{label}</span>
      <div className={styles.shortcutKeys}>
        {combo !== defaultCombo && (
          <button className={styles.shortcutReset} onClick={reset} aria-label="Reset to default">
            <RotateCcw size={13} strokeWidth={2} />
          </button>
        )}
        <button
          onClick={startListening}
          className={`${styles.shortcutCombo} ${listening ? styles.listening : ''}`}
        >
          {listening ? 'Press keys...' : formatCombo(combo)}
        </button>
      </div>
    </div>
  );
};

type Section = 'general' | 'appearance' | 'terminal' | 'shortcuts';

const SECTIONS: { id: Section; label: string; Icon: LucideIcon }[] = [
  { id: 'general', label: 'General', Icon: SlidersHorizontal },
  { id: 'appearance', label: 'Appearance', Icon: Palette },
  { id: 'terminal', label: 'Terminal', Icon: SquareTerminal },
  { id: 'shortcuts', label: 'Shortcuts', Icon: Keyboard }
];

const GROUPS = [...new Set(KEYBINDINGS.map((k) => k.group))];

const dotStyle = (corner: NotifCorner): React.CSSProperties => {
  const [y, x] = corner.split('-');
  return { [y]: 3, [x]: 3 };
};

const THEMES: { id: ThemePref; label: string; description: string }[] = [
  { id: 'system', label: 'System', description: 'Follow the operating system setting.' },
  { id: 'dark', label: 'Dark', description: 'Dark surfaces across the app.' },
  { id: 'light', label: 'Light', description: 'Light surfaces across the app.' }
];

const Settings = ({ onClose }: SettingsProps) => {
  const options = React.useMemo(listTerminalTargets, []);
  const [section, setSection] = React.useState<Section>('general');
  const [target, setTarget] = React.useState(() => getSetting(TERMINAL_TARGET_KEY, 'auto'));
  const [theme, setTheme] = React.useState<ThemePref>(getThemePref);
  const [maxZoom, setMaxZoom] = React.useState(() => getSetting(MAX_ZOOM_KEY, 1));
  const [framePad, setFramePad] = React.useState(() => getSetting(FRAME_PAD_KEY, 0));
  const [headerParts, setHeaderParts] = React.useState(getHeaderParts);
  const [minimapPinned, setPinned] = React.useState(getMinimapPinned);
  const [minimapCorner, setCorner] = React.useState(getMinimapCorner);
  const [monitors, setMonitors] = React.useState<MonitorInfo[]>([]);
  const [placement, setPlacement] = React.useState(getNotifPlacement);

  React.useEffect(() => {
    void listMonitors().then(setMonitors).catch(() => {});
  }, []);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [onClose]);

  const selectTarget = (id: string) => {
    setTarget(id);
    void setSetting(TERMINAL_TARGET_KEY, id);
  };

  const selectTheme = (pref: ThemePref) => {
    setTheme(pref);
    setThemePref(pref);
  };

  const toggleMinimapPinned = () => {
    setMinimapPinned(!minimapPinned);
    setPinned(!minimapPinned);
  };

  const selectMinimapCorner = (corner: MinimapCorner) => {
    setCorner(corner);
    setMinimapCorner(corner);
  };

  const selectCorner = (corner: NotifCorner) => {
    const next = { ...placement, corner };
    setPlacement(next);
    setNotifPlacement(next);
  };

  const selectMonitor = (monitor: string | null) => {
    const next = { ...placement, monitor };
    setPlacement(next);
    setNotifPlacement(next);
  };

  const toggleHeaderPart = (id: HeaderPart) => {
    setHeaderPart(id, !headerParts[id]);
    setHeaderParts(getHeaderParts());
  };

  const changeMaxZoom = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = Number(e.target.value);
    setMaxZoom(value);
    void setSetting(MAX_ZOOM_KEY, value);
  };

  const changeFramePad = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = Math.max(0, Number(e.target.value) || 0);
    setFramePad(value);
    void setSetting(FRAME_PAD_KEY, value);
  };

  return (
    <div className={styles.backdrop} onMouseDown={onClose}>
      <div className={styles.panel} onMouseDown={(e) => e.stopPropagation()}>
        <aside className={styles.sidebar}>
          <h1 className={styles.heading}>Settings</h1>
          <nav className={styles.nav}>
            {SECTIONS.map(({ id, label, Icon }) => (
              <button
                key={id}
                type="button"
                onClick={() => setSection(id)}
                className={`${styles.navItem} ${section === id ? styles.navActive : ''}`}
              >
                <Icon size={15} strokeWidth={1.75} />
                <span>{label}</span>
              </button>
            ))}
          </nav>
        </aside>
        <section className={styles.content}>
          <button className={styles.close} onClick={onClose} aria-label="Close settings">
            <X size={15} strokeWidth={1.75} />
          </button>
          {section === 'general' && (
            <div className={styles.pane}>
              <div className={styles.paneHead}>
                <h2 className={styles.title}>General</h2>
                <p className={styles.subtitle}>Canvas behavior and navigation aids.</p>
              </div>
              <div className={styles.group}>
                <div className={styles.sliderHead}>
                  <p className={styles.groupLabel}>Maximum zoom</p>
                  <span className={styles.sliderValue}>{Math.round(maxZoom * 100)}%</span>
                </div>
                <input
                  min={1}
                  step={0.05}
                  type="range"
                  max={ZOOM_MAX}
                  value={maxZoom}
                  onChange={changeMaxZoom}
                  className={styles.slider}
                />
                <p className={styles.hint}>Above 100% terminal text may look blurry.</p>
              </div>
              <div className={styles.group}>
                <div className={styles.sliderHead}>
                  <p className={styles.groupLabel}>Frame fit padding</p>
                  <input
                    min={0}
                    type="number"
                    value={framePad}
                    onChange={changeFramePad}
                    className={styles.numberInput}
                  />
                </div>
                <p className={styles.hint}>Space in px kept around tiles when fitting a frame to its contents.</p>
              </div>
              <div className={styles.group}>
                <p className={styles.groupLabel}>Minimap</p>
                <div className={styles.options}>
                  <ToggleOption
                    checked={minimapPinned}
                    label="Always visible"
                    onToggle={toggleMinimapPinned}
                    description="Keep the minimap on screen instead of fading out when idle."
                  />
                </div>
                <div className={styles.corners}>
                  {NOTIF_CORNERS.map(({ id, label }) => (
                    <button
                      key={id}
                      type="button"
                      onClick={() => selectMinimapCorner(id)}
                      className={`${styles.corner} ${minimapCorner === id ? styles.selected : ''}`}
                    >
                      <span className={styles.cornerBox}>
                        <span className={styles.cornerDot} style={dotStyle(id)} />
                      </span>
                      <span className={styles.optionLabel}>{label}</span>
                    </button>
                  ))}
                </div>
              </div>
              <div className={styles.group}>
                <p className={styles.groupLabel}>Notification corner</p>
                <div className={styles.corners}>
                  {NOTIF_CORNERS.map(({ id, label }) => (
                    <button
                      key={id}
                      type="button"
                      onClick={() => selectCorner(id)}
                      className={`${styles.corner} ${placement.corner === id ? styles.selected : ''}`}
                    >
                      <span className={styles.cornerBox}>
                        <span className={styles.cornerDot} style={dotStyle(id)} />
                      </span>
                      <span className={styles.optionLabel}>{label}</span>
                    </button>
                  ))}
                </div>
              </div>
              <div className={styles.group}>
                <p className={styles.groupLabel}>Notification display</p>
                <div className={styles.options}>
                  <RadioOption
                    label="Follow main window"
                    selected={placement.monitor === null}
                    onSelect={() => selectMonitor(null)}
                    description="Show toasts on whichever display Panorama is on."
                  />
                  {monitors.map((monitor) => (
                    <RadioOption
                      key={monitor.name}
                      label={monitorLabel(monitor.name)}
                      selected={placement.monitor === monitor.name}
                      onSelect={() => selectMonitor(monitor.name)}
                      description={`${monitor.width}x${monitor.height}${monitor.primary ? ' - primary' : ''}`}
                    />
                  ))}
                </div>
              </div>
            </div>
          )}
          {section === 'terminal' && (
            <div className={styles.pane}>
              <div className={styles.paneHead}>
                <h2 className={styles.title}>Terminal</h2>
                <p className={styles.subtitle}>Changes take effect for new terminals.</p>
              </div>
              <div className={styles.group}>
                <p className={styles.groupLabel}>Terminal target</p>
                <div className={styles.options}>
                  {options.map(({ id, label, isDefault }) => (
                    <RadioOption
                      key={id}
                      label={label}
                      selected={target === id}
                      description={
                        isDefault ? 'Recommended default for this platform.' : 'Available for new terminals.'
                      }
                      onSelect={() => selectTarget(id)}
                    />
                  ))}
                </div>
              </div>
            </div>
          )}
          {section === 'appearance' && (
            <div className={styles.pane}>
              <div className={styles.paneHead}>
                <h2 className={styles.title}>Appearance</h2>
                <p className={styles.subtitle}>Theme applies across the whole app.</p>
              </div>
              <div className={styles.group}>
                <p className={styles.groupLabel}>Theme</p>
                <div className={styles.options}>
                  {THEMES.map(({ id, label, description }) => (
                    <RadioOption
                      key={id}
                      label={label}
                      description={description}
                      selected={theme === id}
                      onSelect={() => selectTheme(id)}
                    />
                  ))}
                </div>
              </div>
              <div className={styles.group}>
                <p className={styles.groupLabel}>Tile header</p>
                <div className={styles.options}>
                  {HEADER_PART_OPTIONS.map(({ id, label, description }) => (
                    <ToggleOption
                      key={id}
                      label={label}
                      description={description}
                      checked={headerParts[id]}
                      onToggle={() => toggleHeaderPart(id)}
                    />
                  ))}
                </div>
              </div>
            </div>
          )}
          {section === 'shortcuts' && (
            <div className={styles.pane}>
              <div className={styles.paneHead}>
                <h2 className={styles.title}>Shortcuts</h2>
                <p className={styles.subtitle}>Click a shortcut and press the keys to rebind it.</p>
              </div>
              {GROUPS.map((group) => (
                <div key={group} className={styles.group}>
                  <p className={styles.groupLabel}>{group}</p>
                  <div className={styles.shortcuts}>
                    {KEYBINDINGS.filter((k) => k.group === group).map((k) => (
                      <ShortcutRow key={k.id} id={k.id} label={k.label} defaultCombo={k.defaultCombo} />
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
};

export default Settings;
