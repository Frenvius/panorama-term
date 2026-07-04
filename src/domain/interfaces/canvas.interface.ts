export type { Tile, Frame } from '~/domain/interfaces/workspace.interface';

export interface View {
  x: number;
  y: number;
  k: number;
}

export interface FrameMember {
  id: string;
  x: number;
  y: number;
}
