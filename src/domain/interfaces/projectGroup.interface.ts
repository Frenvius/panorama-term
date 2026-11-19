import type { Tile } from '~/domain/interfaces/workspace.interface';

export type ProjectGroupKind = 'frame' | 'path' | 'loose';

export interface ProjectGroup {
  key: string; // `frame:<id>` | `path:<normalized path>` | 'loose'
  tiles: Tile[];
  title: string;
  outside: Tile[];
  drifted: string[];
  path: string | null;
  kind: ProjectGroupKind;
  frameId: string | null;
}
