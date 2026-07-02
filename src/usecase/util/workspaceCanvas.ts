import type { View } from '~/domain/interfaces/canvas.interface';
import type { Tile, Frame, CanvasState, Viewport } from '~/domain/interfaces/workspace.interface';

export interface RuntimeCanvas {
  view: View;
  tiles: Tile[];
  frames: Frame[];
}

const viewportSize = (): { w: number; h: number } => ({ w: window.innerWidth, h: window.innerHeight });

const viewFromViewport = (vp: Viewport): View => {
  const { w, h } = viewportSize();
  return { k: vp.zoom, x: w / 2 - vp.centerX * vp.zoom, y: h / 2 - vp.centerY * vp.zoom };
};

const viewportFromView = (view: View): Viewport => {
  const { w, h } = viewportSize();
  return { zoom: view.k, centerX: (w / 2 - view.x) / view.k, centerY: (h / 2 - view.y) / view.k };
};

export const toRuntime = (state: CanvasState): RuntimeCanvas => ({
  tiles: state.tiles ?? [],
  frames: state.frames ?? [],
  view: viewFromViewport(state.viewport)
});

export const toStored = (canvas: RuntimeCanvas): CanvasState => ({
  version: 1,
  tiles: canvas.tiles,
  frames: canvas.frames,
  viewport: viewportFromView(canvas.view)
});
