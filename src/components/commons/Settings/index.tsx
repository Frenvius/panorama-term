import React from 'react';
import { X, Palette, Keyboard, RotateCcw, SquareTerminal } from 'lucide-react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { ZOOM_MAX, MAX_ZOOM_KEY, FRAME_PAD_KEY } from '~/usecase/util/constants';
import { getThemePref, setThemePref, type ThemePref } from '~/usecase/util/theme';
import { listTerminalTargets, TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
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

const GROUPS = [...new Set(KEYBINDINGS.map((k) => k.group))];

const THEMES: { id: ThemePref; label: string; description: string }[] = [
  { id: 'system', label: 'System', description: 'Follow the operating system setting.' },
  { id: 'dark', label: 'Dark', description: 'Dark surfaces across the app.' },
  { id: 'light', label: 'Light', description: 'Light surfaces across the app.' }
];

const Settings = ({ onClose }: SettingsProps) => {
  const options = React.useMemo(listTerminalTargets, []);
  const [section, setSection] = React.useState<'terminal' | 'appearance' | 'shortcuts'>('appearance');
  const [target, setTarget] = React.useState(() => getSetting(TERMINAL_TARGET_KEY, 'auto'));
  const [theme, setTheme] = React.useState<ThemePref>(getThemePref);
  const [maxZoom, setMaxZoom] = React.useState(() => getSetting(MAX_ZOOM_KEY, 1));
  const [framePad, setFramePad] = React.useState(() => getSetting(FRAME_PAD_KEY, 0));

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
            <button
              type="button"
              onClick={() => setSection('appearance')}
              className={`${styles.navItem} ${section === 'appearance' ? styles.navActive : ''}`}
            >
              <Palette size={15} strokeWidth={1.75} />
              <span>Appearance</span>
            </button>
            <button
              type="button"
              onClick={() => setSection('terminal')}
              className={`${styles.navItem} ${section === 'terminal' ? styles.navActive : ''}`}
            >
              <SquareTerminal size={15} strokeWidth={1.75} />
              <span>Terminal</span>
            </button>
            <button
              type="button"
              onClick={() => setSection('shortcuts')}
              className={`${styles.navItem} ${section === 'shortcuts' ? styles.navActive : ''}`}
            >
              <Keyboard size={15} strokeWidth={1.75} />
              <span>Shortcuts</span>
            </button>
          </nav>
        </aside>
        <section className={styles.content}>
          <button className={styles.close} onClick={onClose} aria-label="Close settings">
            <X size={15} strokeWidth={1.75} />
          </button>
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
