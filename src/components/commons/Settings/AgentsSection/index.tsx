import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { USAGE_PROVIDERS, IDLE_COMPACTION_KEY } from '~/usecase/util/constants';
import { PROVIDER_NAMES } from '~/components/commons/StatusBar/constants';
import { useUsageProviders, toggleUsageProvider } from '~/usecase/hooks/useUsageProviders';
import { setProviderHidden, getSeenProviders, getHiddenProviders } from '~/usecase/util/agentProviders';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Switch from '~/components/commons/Settings/Switch';
import Select from '~/components/commons/Settings/Select';
import {
  type TasksOpen,
  type TasksLink,
  type TasksDetail,
  getTasksOpen,
  getTasksLink,
  getTasksAuto,
  getTasksDetail,
  setTasksOpen,
  setTasksLink,
  setTasksAuto,
  setTasksDetail
} from '~/usecase/util/bgTasks';

const TASKS_OPEN_OPTIONS: { value: TasksOpen; label: string }[] = [
  { value: 'inline', label: 'In the agent bar' },
  { value: 'auto', label: 'Tile, first free side' },
  { value: 'right', label: 'Tile on the right' },
  { value: 'left', label: 'Tile on the left' },
  { value: 'above', label: 'Tile above' },
  { value: 'below', label: 'Tile below' }
];

const TASKS_LINK_OPTIONS: { value: TasksLink; label: string }[] = [
  { value: 'dashed', label: 'Dashed' },
  { value: 'solid', label: 'Solid' },
  { value: 'highlight', label: 'Highlighted' },
  { value: 'none', label: 'None' }
];

const TASKS_DETAIL_OPTIONS: { value: TasksDetail; label: string }[] = [
  { value: 'compact', label: 'Compact' },
  { value: 'normal', label: 'Normal' },
  { value: 'full', label: 'Full' }
];

const AgentsSection = () => {
  const [idleCompact, setIdleCompact] = React.useState(() => getSetting(IDLE_COMPACTION_KEY, true));
  const [providers] = React.useState(getSeenProviders);
  const [hiddenProviders, setHiddenProviders] = React.useState(getHiddenProviders);
  const usageProviders = useUsageProviders();
  const [tasksOpen, setOpen] = React.useState(getTasksOpen);
  const [tasksLink, setLink] = React.useState(getTasksLink);
  const [tasksAuto, setAuto] = React.useState(getTasksAuto);
  const [tasksDetail, setDetail] = React.useState(getTasksDetail);

  const toggleTasksAuto = () => {
    setAuto(!tasksAuto);
    setTasksAuto(!tasksAuto);
  };

  const pickTasksDetail = (value: string) => {
    setDetail(value as TasksDetail);
    setTasksDetail(value as TasksDetail);
  };

  const pickTasksOpen = (value: string) => {
    setOpen(value as TasksOpen);
    setTasksOpen(value as TasksOpen);
  };

  const pickTasksLink = (value: string) => {
    setLink(value as TasksLink);
    setTasksLink(value as TasksLink);
  };

  const toggleIdleCompact = () => {
    const next = !idleCompact;
    setIdleCompact(next);
    void setSetting(IDLE_COMPACTION_KEY, next);
  };

  const toggleProvider = (provider: string) => {
    const hidden = !hiddenProviders.includes(provider);
    setProviderHidden(provider, hidden);
    setHiddenProviders(getHiddenProviders());
  };

  return (
    <>
      <Group label="Model picker">
        {providers.length === 0 ? (
          <Row settingId="agents.modelPicker" label="No providers seen yet" description="Open a terminal running an agent that reports its models." />
        ) : (
          providers.map((provider) => {
            const handleToggle = () => toggleProvider(provider);
            return (
              <Row key={provider} label={provider} description="List this provider's models in the model picker.">
                <Switch
                  checked={!hiddenProviders.includes(provider)}
                  onChange={handleToggle}
                  label={provider}
                />
              </Row>
            );
          })
        )}
      </Group>
      <Group label="Status bar usage">
        {USAGE_PROVIDERS.map((provider, i) => {
          const handleToggle = () => toggleUsageProvider(provider);
          return (
            <Row
              key={provider}
              settingId={i === 0 ? 'agents.usage' : undefined}
              label={PROVIDER_NAMES[provider]}
              description="Show this provider's usage limits in the status bar."
            >
              <Switch checked={usageProviders.includes(provider)} onChange={handleToggle} label={PROVIDER_NAMES[provider]} />
            </Row>
          );
        })}
      </Group>
      <Group label="Claude Code">
        <Row
          settingId="agents.idleCompaction"
          label="Idle compaction"
          description="Compact conversations over 200K tokens after about an hour idle, before the prompt cache expires."
        >
          <Switch checked={idleCompact} onChange={toggleIdleCompact} label="Idle compaction" />
        </Row>
      </Group>
      <Group label="Background tasks">
        <Row
          settingId="agents.tasksOpen"
          label="Open"
          description="Where the tasks chip shows what the agent runs: subagents, background shells and monitors, and other agent CLIs such as Codex. A tile can be moved, resized and closed."
        >
          <Select options={TASKS_OPEN_OPTIONS} value={tasksOpen} onChange={pickTasksOpen} />
        </Row>
        <Row
          settingId="agents.tasksAuto"
          label="Open automatically"
          description="Open the tile beside the terminal when a task starts, without taking focus."
          disabled={tasksOpen === 'inline'}
        >
          <Switch checked={tasksAuto} onChange={toggleTasksAuto} label="Open automatically" disabled={tasksOpen === 'inline'} />
        </Row>
        <Row
          settingId="agents.tasksDetail"
          label="Detail"
          description="Compact: title, status and time. Normal: plus what each task is doing now. Full: every running task expanded with its live output, Codex reasoning included."
          disabled={tasksOpen === 'inline'}
        >
          <Select options={TASKS_DETAIL_OPTIONS} value={tasksDetail} onChange={pickTasksDetail} disabled={tasksOpen === 'inline'} />
        </Row>
        <Row settingId="agents.tasksLink" label="Link" description="Line between the terminal and its tasks tile." disabled={tasksOpen === 'inline'}>
          <Select options={TASKS_LINK_OPTIONS} value={tasksLink} onChange={pickTasksLink} disabled={tasksOpen === 'inline'} />
        </Row>
      </Group>
    </>
  );
};

export default AgentsSection;
