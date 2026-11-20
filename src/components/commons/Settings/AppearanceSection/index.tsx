import type { FontUse, ThemeKind } from '~/domain/interfaces/plugin.interface';

import React from 'react';

import { usePlugins } from '~/usecase/hooks/usePlugins';
import { enabledManifests } from '~/usecase/util/plugins';
import { fontChoices, getFontChoice, setFontChoice } from '~/usecase/util/fonts';
import { setHeaderPart, getHeaderParts, HEADER_PART_OPTIONS, type HeaderPart } from '~/usecase/util/headerParts';
import { themeChoices, getThemePref, setThemePref, getThemeChoice, setThemeChoice, type ThemePref } from '~/usecase/util/theme';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Select from '~/components/commons/Settings/Select';
import Switch from '~/components/commons/Settings/Switch';
import Segmented from '~/components/commons/Settings/Segmented';
import { SINGLE_FONT_HINT, SINGLE_THEME_HINT } from '~/components/commons/Settings/constants';

const THEME_OPTIONS = [
  { value: 'system', label: 'System' },
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' }
];

const THEME_DESC: Record<ThemePref, string> = {
  system: 'Follow the operating system setting.',
  dark: 'Dark surfaces across the app.',
  light: 'Light surfaces across the app.'
};

const THEME_KINDS: { kind: ThemeKind; label: string; settingId: string }[] = [
  { kind: 'dark', label: 'Dark theme', settingId: 'appearance.darkTheme' },
  { kind: 'light', label: 'Light theme', settingId: 'appearance.lightTheme' }
];

const FONT_GROUPS: { use: 'ui' | 'code'; label: string; settingId: string }[] = [
  { use: 'ui', label: 'Interface font', settingId: 'appearance.interfaceFont' },
  { use: 'code', label: 'Code font', settingId: 'appearance.codeFont' }
];

const AppearanceSection = () => {
  const { plugins } = usePlugins();
  const installed = React.useMemo(() => enabledManifests(plugins), [plugins]);
  const [theme, setTheme] = React.useState<ThemePref>(getThemePref);
  const [headerParts, setHeaderParts] = React.useState(getHeaderParts);
  const [themeKeys, setThemeKeys] = React.useState(() => ({
    dark: getThemeChoice('dark'),
    light: getThemeChoice('light')
  }));
  const [fontKeys, setFontKeys] = React.useState(() => ({
    ui: getFontChoice('ui'),
    code: getFontChoice('code')
  }));

  const selectTheme = (value: string) => {
    const pref = value as ThemePref;
    setTheme(pref);
    setThemePref(pref);
  };

  const selectThemeChoice = (kind: ThemeKind, key: string) => {
    setThemeKeys((keys) => ({ ...keys, [kind]: key }));
    setThemeChoice(kind, key);
  };

  const selectFontChoice = (use: FontUse, key: string) => {
    setFontKeys((keys) => ({ ...keys, [use]: key }));
    void setFontChoice(use, key);
  };

  const toggleHeaderPart = (id: HeaderPart) => {
    setHeaderPart(id, !headerParts[id]);
    setHeaderParts(getHeaderParts());
  };

  return (
    <>
      <Group label="Theme">
        <Row settingId="appearance.theme" label="Mode" description={THEME_DESC[theme]}>
          <Segmented options={THEME_OPTIONS} value={theme} onChange={selectTheme} label="Theme mode" />
        </Row>
        {THEME_KINDS.map(({ kind, label, settingId }) => {
          const choices = themeChoices(installed, kind);
          const single = choices.length < 2;
          const handleChange = (key: string) => selectThemeChoice(kind, key);
          return (
            <Row key={kind} settingId={settingId} label={label} description={single ? SINGLE_THEME_HINT : undefined}>
              <Select
                disabled={single}
                options={choices.map((c) => ({ value: c.key, label: c.label, secondary: c.description }))}
                value={themeKeys[kind]}
                onChange={handleChange}
              />
            </Row>
          );
        })}
      </Group>
      <Group label="Fonts">
        {FONT_GROUPS.map(({ use, label, settingId }) => {
          const choices = fontChoices(installed, use);
          const single = choices.length < 2;
          const handleChange = (key: string) => selectFontChoice(use, key);
          return (
            <Row key={use} settingId={settingId} label={label} description={single ? SINGLE_FONT_HINT : undefined}>
              <Select
                disabled={single}
                options={choices.map((c) => ({ value: c.key, label: c.label, secondary: c.description }))}
                value={fontKeys[use]}
                onChange={handleChange}
              />
            </Row>
          );
        })}
      </Group>
      <Group label="Tile header">
        {HEADER_PART_OPTIONS.map(({ id, label, description }) => {
          const handleToggle = () => toggleHeaderPart(id);
          return (
            <Row key={id} settingId={`appearance.tileHeader.${id}`} label={label} description={description}>
              <Switch checked={headerParts[id]} onChange={handleToggle} label={label} />
            </Row>
          );
        })}
      </Group>
    </>
  );
};

export default AppearanceSection;
