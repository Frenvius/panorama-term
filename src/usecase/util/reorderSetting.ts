import type { Spacing, ReorderMode } from '~/usecase/util/reorderLayout';

import { getSetting } from '~/adapter/settings/settings.client';
import { isReorderMode, layoutSpacing } from '~/usecase/util/reorderLayout';
import { FRAME_PAD_KEY, REORDER_MODE_KEY, FRAME_PAD_DEFAULT } from '~/usecase/util/constants';

const currentReorderMode = (): ReorderMode => {
  const mode = getSetting<unknown>(REORDER_MODE_KEY, 'default');
  return isReorderMode(mode) ? mode : 'default';
};

export const currentLayoutSpacing = (): Spacing =>
  layoutSpacing(currentReorderMode(), Math.max(0, getSetting(FRAME_PAD_KEY, FRAME_PAD_DEFAULT)));
