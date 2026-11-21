import type { Tile, Frame } from '~/domain/interfaces/workspace.interface';
import type { TidyPlan } from '~/domain/interfaces/orch.interface';
import type { ClusterPlanner } from '~/usecase/util/tidy';

import React from 'react';

import { aiTidy, aiProgress } from '~/adapter/orch/orch.client';
import { showToast, errorText } from '~/usecase/util/toasts';
import { pinPathlessNotes, settleTidyLayout, buildTidySnapshot, sanitizeTidyPlan, computeTidyLayout } from '~/usecase/util/tidy';

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
  progress: string | null;
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
  patchTile: (id: string, patch: Partial<Tile>) => void;
  setProjectsOrder: (paths: string[]) => void;
  planCluster?: ClusterPlanner;
}

export const useTidyAI = (opts: UseTidyAIOpts): TidyState => {
  const [status, setStatus] = React.useState<TidyStatus>('idle');
  const [preview, setPreview] = React.useState<TidyPreview | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  const [progress, setProgress] = React.useState<string | null>(null);

  const optsRef = React.useRef(opts);
  optsRef.current = opts;

  React.useEffect(() => {
    if (status !== 'loading') {
      setProgress(null);
      return;
    }
    const startedAt = Date.now();
    let step = 'Starting';
    const poll = () => {
      setProgress(`${step} (${Math.round((Date.now() - startedAt) / 1000)}s)`);
      void aiProgress('tidy').then(
        (p) => {
          if (p) step = p.step;
        },
        () => {}
      );
    };
    poll();
    const timer = window.setInterval(poll, 1000);
    return () => window.clearInterval(timer);
  }, [status]);

  const onTrigger = React.useCallback(() => {
    if (optsRef.current.projects.length === 0) return;
    setStatus('loading');
    setError(null);

    const { tiles, frames, projects } = optsRef.current;
    const snapshot = buildTidySnapshot({ tiles, frames, projects });

    void aiTidy(snapshot).then(
      (rawPlan) => {
        const plan = sanitizeTidyPlan(rawPlan, { projectPaths: projects, tiles, frames });
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

    const { tiles, frames, createFrame, patchFrame, removeFrame, patchTile, setProjectsOrder, projects, planCluster } = optsRef.current;

    const undoTileRects = tiles.map((t) => ({ id: t.id, rect: { x: t.x, y: t.y, width: t.width, height: t.height } }));
    const undoFrameStates = frames.map((f) => ({ ...f }));
    const undoProjectOrder = [...projects];

    setStatus('applying');

    const notePins = pinPathlessNotes({ projects, frames, tiles });
    for (const pin of notePins) patchTile(pin.id, { workspacePath: pin.workspacePath });

    const layout = computeTidyLayout(p.plan, tiles, frames, projects);
    const settled = settleTidyLayout(layout, tiles, frames, planCluster);
    for (const [id, rect] of settled.tileRects) patchTile(id, rect);
    for (const fp of settled.framePatches) patchFrame(fp.id, fp.patch);
    for (const id of settled.removedFrameIds) removeFrame(id);
    const createdFrameIds = settled.newFrames.map((nf) => createFrame(nf));
    const removedFrames = undoFrameStates.filter((f) => settled.removedFrameIds.includes(f.id));

    setProjectsOrder(p.plan.projectOrder);
    setStatus('idle');
    setPreview(null);

    showToast({
      kind: 'success',
      message: 'Tidy applied',
      action: {
        label: 'Undo',
        run: () => {
          const ops = optsRef.current;
          ops.setProjectsOrder(undoProjectOrder);
          for (const { id, rect } of undoTileRects) {
            if (settled.tileRects.has(id)) ops.patchTile(id, rect);
          }
          for (const pin of notePins) ops.patchTile(pin.id, { workspacePath: undefined });
          for (const id of createdFrameIds) ops.removeFrame(id);
          for (const { id: _id, ...frame } of removedFrames) ops.createFrame(frame);
          for (const f of undoFrameStates) {
            if (settled.removedFrameIds.includes(f.id)) continue;
            ops.patchFrame(f.id, { x: f.x, y: f.y, width: f.width, height: f.height, title: f.title, color: f.color, path: f.path });
          }
        }
      }
    });
  }, [preview]);

  const onCancel = React.useCallback(() => {
    setStatus('idle');
    setPreview(null);
  }, []);

  return React.useMemo(
    () => ({ status, preview, error, progress, onTrigger, onApply, onCancel }),
    [status, preview, error, progress, onTrigger, onApply, onCancel]
  );
};
