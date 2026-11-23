import React from 'react';

import { getSetting, setSetting } from '~/adapter/settings/settings.client';
import { getTabScope, setTabScope, subscribeTabScope } from '~/usecase/util/tabScope';
import { isReorderMode, type ReorderMode } from '~/usecase/util/reorderLayout';
import { getMinimapCorner, getMinimapPinned, setMinimapCorner, setMinimapPinned, type MinimapCorner } from '~/usecase/util/minimap';
import { getCanvasSidebarMode, setCanvasSidebarMode, CANVAS_SIDEBAR_MODE_EVENT, type CanvasSidebarMode } from '~/usecase/util/canvasSidebars';
import { ZOOM_MAX, MAX_ZOOM_KEY, FRAME_PAD_KEY, MAX_ZOOM_FLOOR, REORDER_MODE_KEY, FRAME_PAD_MAX, FRAME_PAD_DEFAULT } from '~/usecase/util/constants';
import Group from '~/components/commons/Settings/Group';
import Row from '~/components/commons/Settings/Row';
import Slider from '~/components/commons/Settings/Slider';
import Switch from '~/components/commons/Settings/Switch';
import Segmented from '~/components/commons/Settings/Segmented';
import CornerPicker from '~/components/commons/Settings/CornerPicker';

const REORDER_OPTIONS = [
  { value: 'default', label: 'Spaced' },
  { value: 'compact', label: 'Compact' }
];

const REORDER_DESC: Record<ReorderMode, string> = {
  default: 'Leaves a gap between tiles and padding inside frames.',
  compact: 'Packs tiles edge to edge and wraps frames tightly.'
};

const SIDEBAR_OPTIONS = [
  { value: 'overlay', label: 'Float' },
  { value: 'docked', label: 'Beside' }
];

const SIDEBAR_DESC: Record<CanvasSidebarMode, string> = {
  overlay: 'Sidebars float above the canvas surface.',
  docked: 'Beside canvas shrinks the canvas to the space between the sidebars.'
};

const CanvasSection = () => {
  const tabScope = React.useSyncExternalStore(subscribeTabScope, getTabScope);
  const [maxZoom, setMaxZoom] = React.useState(() => getSetting(MAX_ZOOM_KEY, 1));
  const [framePad, setFramePad] = React.useState(() => getSetting(FRAME_PAD_KEY, FRAME_PAD_DEFAULT));
  const [reorderMode, setReorderMode] = React.useState<ReorderMode>(() => {
    const mode = getSetting<unknown>(REORDER_MODE_KEY, 'default');
    return isReorderMode(mode) ? mode : 'default';
  });
  const [minimapPinned, setMinimapPinnedState] = React.useState(getMinimapPinned);
  const [minimapCorner, setMinimapCornerState] = React.useState<MinimapCorner>(getMinimapCorner);
  const [sidebarMode, setSidebarMode] = React.useState<CanvasSidebarMode>(getCanvasSidebarMode);

  React.useEffect(() => {
    const onMode = () => setSidebarMode(getCanvasSidebarMode());
    window.addEventListener(CANVAS_SIDEBAR_MODE_EVENT, onMode);
    return () => window.removeEventListener(CANVAS_SIDEBAR_MODE_EVENT, onMode);
  }, []);

  const changeMaxZoom = (value: number) => {
    setMaxZoom(value);
    void setSetting(MAX_ZOOM_KEY, value);
  };

  const changeFramePad = (value: number) => {
    setFramePad(value);
    void setSetting(FRAME_PAD_KEY, value);
  };

  const selectReorderMode = (mode: string) => {
    if (!isReorderMode(mode)) return;
    setReorderMode(mode);
    void setSetting(REORDER_MODE_KEY, mode);
  };

  const toggleMinimapPinned = () => {
    const next = !minimapPinned;
    setMinimapPinnedState(next);
    setMinimapPinned(next);
  };

  const selectMinimapCorner = (corner: MinimapCorner) => {
    setMinimapCornerState(corner);
    setMinimapCorner(corner);
  };

  const selectSidebarMode = (mode: string) => {
    const m = mode as CanvasSidebarMode;
    setSidebarMode(m);
    setCanvasSidebarMode(m);
  };

  return (
    <>
      <Group label="Tabs">
        <Row settingId="canvas.tabScope" label="Tab scope" description="Show tabs for the current project or all projects.">
          <Segmented options={[{ value: 'project', label: 'Per project' }, { value: 'global', label: 'Global' }]} value={tabScope} onChange={(value) => setTabScope(value === 'global' ? 'global' : 'project')} label="Tab scope" />
        </Row>
      </Group>
      <Group label="Zoom">
        <Row
          settingId="canvas.maxZoom"
          label="Maximum zoom"
          description="Above 100% terminal text may look blurry."
          stacked
        >
          <Slider
            min={MAX_ZOOM_FLOOR}
            max={ZOOM_MAX}
            step={0.05}
            value={maxZoom}
            onChange={changeMaxZoom}
            formatValue={(v) => `${Math.round(v * 100)}%`}
          />
        </Row>
      </Group>
      <Group label="Layout">
        <Row
          settingId="canvas.reorderMode"
          label="Reorder layout"
          description={REORDER_DESC[reorderMode]}
        >
          <Segmented options={REORDER_OPTIONS} value={reorderMode} onChange={selectReorderMode} label="Reorder layout" />
        </Row>
        <Row
          settingId="canvas.gap"
          label="Gap"
          description="Space between tiles and inside frames. Compact always uses 0."
          disabled={reorderMode === 'compact'}
          stacked
        >
          <Slider
            min={0}
            max={FRAME_PAD_MAX}
            step={2}
            value={framePad}
            onChange={changeFramePad}
            disabled={reorderMode === 'compact'}
            formatValue={(v) => `${v}px`}
          />
        </Row>
      </Group>
      <Group label="Minimap">
        <Row
          settingId="canvas.minimapPinned"
          label="Always visible"
          description="Keep the minimap on screen instead of fading out when idle."
        >
          <Switch checked={minimapPinned} onChange={toggleMinimapPinned} label="Always visible" />
        </Row>
        <Row settingId="canvas.minimapCorner" label="Position">
          <CornerPicker value={minimapCorner} onChange={selectMinimapCorner} label="Minimap position" />
        </Row>
      </Group>
      <Group label="Sidebars">
        <Row
          settingId="canvas.sidebarMode"
          label="Sidebar placement"
          description={SIDEBAR_DESC[sidebarMode]}
        >
          <Segmented options={SIDEBAR_OPTIONS} value={sidebarMode} onChange={selectSidebarMode} label="Sidebar placement" />
        </Row>
      </Group>
    </>
  );
};

export default CanvasSection;
