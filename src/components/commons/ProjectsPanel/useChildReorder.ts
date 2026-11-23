import type { ListReorder } from './useListReorder';
import type { Tile } from '~/domain/interfaces/workspace.interface';

import React from 'react';

import { useListReorder } from './useListReorder';
import { getPref, setPref } from '~/usecase/util/prefs';
import { orderProjectChildren } from '~/usecase/util/projectGroups';

const PREFIX = 'panorama:projectChildren:';

interface ChildReorderOpts {
  groupKey: string;
  children: Tile[];
  enabled: boolean;
}

const readOrder = (key: string): string[] => {
  try {
    const value: unknown = JSON.parse(getPref(PREFIX + key) ?? '[]');
    return Array.isArray(value) ? value.filter((id): id is string => typeof id === 'string') : [];
  } catch {
    return [];
  }
};

export const useChildReorder = ({ groupKey, children, enabled }: ChildReorderOpts) => {
  const [saved, setSaved] = React.useState(() => readOrder(groupKey));
  const ordered = orderProjectChildren(children, saved);

  React.useEffect(() => {
    setSaved(readOrder(groupKey));
  }, [groupKey]);

  const handleReorder = ({ from, to }: ListReorder) => {
    const ids = ordered.map((tile) => tile.id);
    const [id] = ids.splice(from, 1);
    ids.splice(to, 0, id);
    setSaved(ids);
    setPref(PREFIX + groupKey, JSON.stringify(ids));
  };

  return { ordered, handlers: useListReorder({ enabled, onReorder: handleReorder }) };
};
