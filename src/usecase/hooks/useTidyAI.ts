import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { TidyPlan } from '~/domain/interfaces/orch.interface';

import React from 'react';

import { aiTidy } from '~/adapter/orch/orch.client';
import { showToast, errorText } from '~/usecase/util/toasts';
import { buildTidySnapshot, sanitizeTidyPlan, computeTidyLayout } from '~/usecase/util/tidy';

export type TidyStatus = 'idle' | 'loading' | 'preview' | 'applying';

export interface TidyPreview {
  plan: TidyPlan;
  projectsReordered: number;
  framesTotal: number;
  framesNew: number;
}

export interface TidyState {
  status: TidyStatus;
  preview: TidyPreview | null;
  error: string | null;
  onTrigger: () => void;
  onApply: () => void;
  onCancel: () => void;
}

export interface UseTidyAIOpts {
  projects: string[];
  tiles: Tile[];
  frames: Frame[];
  createFrame: (frame: Omit<Frame, 'id'>) => string;
  patchFrame: (id: string, patch: Partial<Omit<Frame, 'id'>>) => void;
  removeFrame: (id: string) => void;
  placeTiles: (moves: { id: string; x: number; y: number }[]) => void;
  setProjectsOrder: (paths: string[]) => void;
}

export const useTidyAI = (opts: UseTidyAIOpts): TidyState => {
  const [status, setStatus] = React.useState<TidyStatus>('idle');
  const [preview, setPreview] = React.useState<TidyPreview | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  const optsRef = React.useRef(opts);
  optsRef.current = opts;

  const onTrigger = React.useCallback(() => {
    if (optsRef.current.projects.length === 0) return;
    setStatus('loading');
    setError(null);

    const { tiles, frames, projects } = optsRef.current;
    const snapshot = buildTidySnapshot({ tiles, frames, projects });

    void aiTidy(snapshot).then(
      (rawPlan) => {
        const plan = sanitizeTidyPlan(rawPlan, { projectPaths: projects, tileIds: tiles.map((t) => t.id) });
        const knownFrameIds = new Set(frames.map((f) => f.id));
        const framesNew = plan.frames.filter((f) => !f.frameId || !knownFrameIds.has(f.frameId)).length;

        const newOrder = plan.projectOrder;
        const currentOrder = projects;
        let projectsReordered = 0;
        for (let i = 0; i < newOrder.length; i++) {
          if (newOrder[i] !== currentOrder[i]) projectsReordered++;
        }

        setPreview({ plan, projectsReordered, framesTotal: plan.frames.length, framesNew });
        setStatus('preview');
      },
      (err: unknown) => {
        setError(errorText(err));
        setStatus('idle');
      }
    );
  }, []);

  const onApply = React.useCallback(() => {
    const p = preview;
    if (!p) return;

    const { tiles, frames, createFrame, patchFrame, placeTiles, setProjectsOrder, projects } = optsRef.current;

    const undoTilePositions = tiles.map((t) => ({ id: t.id, x: t.x, y: t.y }));
    const undoFrameStates = frames.map((f) => ({ ...f }));
    const undoProjectOrder = [...projects];

    setStatus('applying');

    const layout = computeTidyLayout(p.plan, tiles, frames);
    if (layout.tileMoves.length) placeTiles(layout.tileMoves);
    for (const fp of layout.framePatches) patchFrame(fp.id, fp.patch);

    const createdFrameIds: string[] = [];
    for (const nf of layout.newFrames) {
      const id = createFrame(nf);
      createdFrameIds.push(id);
    }

    setProjectsOrder(p.plan.projectOrder);
    setStatus('idle');
    setPreview(null);

    showToast({
      kind: 'success',
      message: 'Tidy applied',
      action: {
        label: 'Undo',
        run: () => {
          optsRef.current.setProjectsOrder(undoProjectOrder);
          optsRef.current.placeTiles(undoTilePositions);
          for (const id of createdFrameIds) optsRef.current.removeFrame(id);
          for (const f of undoFrameStates) {
            if (!createdFrameIds.includes(f.id)) {
              optsRef.current.patchFrame(f.id, {
                x: f.x,
                y: f.y,
                width: f.width,
                height: f.height,
                title: f.title,
                color: f.color,
                path: f.path
              });
            }
          }
        }
      }
    });
  }, [preview]);

  const onCancel = React.useCallback(() => {
    setStatus('idle');
    setPreview(null);
  }, []);

  return { status, preview, error, onTrigger, onApply, onCancel };
};
