import type { FontUse } from '~/domain/interfaces/plugin.interface';

import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { usePlugins } from '~/usecase/hooks/usePlugins';
import { enabledManifests } from '~/usecase/util/plugins';
import { fontChoices, getFontChoice, setFontChoice } from '~/usecase/util/fonts';
import { listTerminalTargets, TERMINAL_TARGET_KEY } from '~/usecase/util/terminalTarget';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Select from '~/components/commons/Settings/Select';
import { SINGLE_FONT_HINT } from '~/components/commons/Settings/constants';

const TERMINAL_FONT_USE: FontUse = 'terminal';

const TerminalSection = () => {
  const options = React.useMemo(listTerminalTargets, []);
  const { plugins } = usePlugins();
  const installed = React.useMemo(() => enabledManifests(plugins), [plugins]);
  const [target, setTarget] = React.useState(() => getSetting(TERMINAL_TARGET_KEY, 'auto'));
  const [fontKey, setFontKey] = React.useState(() => getFontChoice(TERMINAL_FONT_USE));

  const shellOptions = options.map((o) => ({
    value: o.id,
    label: o.label,
    secondary: o.isDefault ? 'Recommended' : undefined
  }));

  const fontOptions = React.useMemo(() => {
    const choices = fontChoices(installed, TERMINAL_FONT_USE);
    return choices.map((c) => ({ value: c.key, label: c.label, secondary: c.description }));
  }, [installed]);

  const selectTarget = (id: string) => {
    setTarget(id);
    void setSetting(TERMINAL_TARGET_KEY, id);
  };

  const selectFont = (key: string) => {
    setFontKey(key);
    void setFontChoice(TERMINAL_FONT_USE, key);
  };

  return (
    <>
      <Group label="Shell">
        <Row settingId="terminal.shell" label="Default shell">
          <Select options={shellOptions} value={target} onChange={selectTarget} />
        </Row>
      </Group>
      <Group label="Font">
        <Row settingId="terminal.font" label="Terminal font" description={fontOptions.length < 2 ? SINGLE_FONT_HINT : undefined}>
          <Select options={fontOptions} value={fontKey} onChange={selectFont} disabled={fontOptions.length < 2} />
        </Row>
      </Group>
    </>
  );
};

export default TerminalSection;
