import type { TileType } from '~/domain/interfaces/workspace.interface';

export type CommandBarKind = 'tile' | 'frame' | 'tab' | 'workspace' | 'command' | 'setting';

export interface CommandBarItem {
  id: string;
  label: string;
  hint?: string;
  run: () => void;
  scope?: string;
  color?: string;
  detail?: string;
  alert?: boolean;
  kind: CommandBarKind;
  tileType?: TileType;
}

export interface CommandBarRow {
  rank: number;
  hits: number[];
  item: CommandBarItem;
}

export interface CommandBarSection {
  title: string;
  rows: CommandBarRow[];
  kind: CommandBarKind | 'recent';
}
