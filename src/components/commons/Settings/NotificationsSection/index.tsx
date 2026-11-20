import type { MonitorInfo } from '~/usecase/util/notifPlacement';

import React from 'react';

import { listMonitors, monitorLabel, getNotifPlacement, setNotifPlacement, type NotifCorner } from '~/usecase/util/notifPlacement';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Select from '~/components/commons/Settings/Select';
import CornerPicker from '~/components/commons/Settings/CornerPicker';

const NotificationsSection = () => {
  const [placement, setPlacement] = React.useState(getNotifPlacement);
  const [monitors, setMonitors] = React.useState<MonitorInfo[]>([]);

  React.useEffect(() => {
    void listMonitors()
      .then(setMonitors)
      .catch(() => {});
  }, []);

  const selectCorner = (corner: NotifCorner) => {
    const next = { ...placement, corner };
    setPlacement(next);
    setNotifPlacement(next);
  };

  const selectMonitor = (value: string) => {
    const monitor = value === '__follow' ? null : value;
    const next = { ...placement, monitor };
    setPlacement(next);
    setNotifPlacement(next);
  };

  const monitorOptions = [
    { value: '__follow', label: 'Follow main window', secondary: 'Show toasts on whichever display Panorama is on.' },
    ...monitors.map((m) => ({
      value: m.name,
      label: monitorLabel(m.name),
      secondary: `${m.width}x${m.height}${m.primary ? ', primary' : ''}`
    }))
  ];
  const savedMonitor = placement.monitor;
  if (savedMonitor && monitors.length > 0 && !monitors.some((m) => m.name === savedMonitor)) {
    monitorOptions.push({ value: savedMonitor, label: monitorLabel(savedMonitor), secondary: 'Not connected' });
  }

  return (
    <>
      <Group label="Toasts">
        <Row settingId="notifications.corner" label="Position">
          <CornerPicker value={placement.corner} onChange={selectCorner} label="Toast position" />
        </Row>
        <Row settingId="notifications.display" label="Display">
          <Select
            options={monitorOptions}
            value={placement.monitor ?? '__follow'}
            onChange={selectMonitor}
          />
        </Row>
      </Group>
    </>
  );
};

export default NotificationsSection;
