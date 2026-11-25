import React from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { watchAppUpdateRequests } from '~/adapter/ui/ui.client';
import { postUpdateStatus } from '~/adapter/ui/appUpdate.client';

export type UpdaterStatus = 'idle' | 'checking' | 'available' | 'up-to-date' | 'downloading' | 'ready' | 'error';

export interface UpdaterState {
  status: UpdaterStatus;
  version: string | null;
  notes: string | null;
  error: string | null;
  downloaded: number;
  contentLength: number;
  currentVersion: string | null;
}

const INITIAL_STATE: UpdaterState = {
  status: 'idle',
  version: null,
  notes: null,
  error: null,
  downloaded: 0,
  contentLength: 0,
  currentVersion: null,
};

export const canUpdate = () => !import.meta.env.DEV && typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let state = INITIAL_STATE;
let pendingUpdate: Update | null = null;
let checking = false;
let bootCheckScheduled = false;
let lastReportKey = '';
const listeners = new Set<() => void>();

const setState = (patch: Partial<UpdaterState>) => {
  state = { ...state, ...patch };
  listeners.forEach((fn) => fn());
  const percent = state.contentLength > 0 ? Math.floor((state.downloaded / state.contentLength) * 10) : 0;
  const reportKey = `${state.status}:${percent}`;
  if (reportKey === lastReportKey) return;
  lastReportKey = reportKey;
  postUpdateStatus(state);
};

const subscribe = (fn: () => void) => {
  listeners.add(fn);
  return () => listeners.delete(fn);
};

const getState = () => state;

const errorMessage = (err: unknown) => (err instanceof Error ? err.message : String(err));

const checkForUpdate = async () => {
  if (!canUpdate() || checking) return;
  checking = true;
  setState({ error: null, status: 'checking' });

  try {
    const result = await check();
    if (!result) {
      pendingUpdate = null;
      setState({ status: 'up-to-date' });
      return;
    }
    pendingUpdate = result;
    setState({
      status: 'available',
      version: result.version,
      notes: result.body ?? null,
      currentVersion: result.currentVersion,
    });
  } catch (err) {
    setState({ status: 'error', error: errorMessage(err) });
  } finally {
    checking = false;
  }
};

const downloadAndInstall = async () => {
  const update = pendingUpdate;
  if (!update) return;
  setState({ error: null, downloaded: 0, contentLength: 0, status: 'downloading' });

  try {
    let downloaded = 0;
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case 'Started':
          setState({ contentLength: event.data.contentLength ?? 0, downloaded: 0 });
          break;
        case 'Progress':
          downloaded += event.data.chunkLength;
          setState({ downloaded });
          break;
        case 'Finished':
          setState({ status: 'ready' });
          break;
      }
    });
  } catch (err) {
    setState({ status: 'error', error: errorMessage(err) });
  }
};

const restart = async () => {
  try {
    await relaunch();
  } catch (err) {
    setState({ status: 'error', error: errorMessage(err) });
  }
};

const dismiss = () => {
  pendingUpdate = null;
  setState(INITIAL_STATE);
};

const scheduleBootCheck = () => {
  if (bootCheckScheduled || !canUpdate()) return;
  bootCheckScheduled = true;
  window.setTimeout(() => void checkForUpdate(), 3000);
};

const reportBootStatus = async (): Promise<void> => {
  if (!canUpdate()) {
    postUpdateStatus({ ...INITIAL_STATE, status: 'error', error: 'updates disabled in dev builds' });
    return;
  }
  try {
    const version = await getVersion();
    postUpdateStatus({ ...INITIAL_STATE, currentVersion: version });
  } catch {
    postUpdateStatus(INITIAL_STATE);
  }
};

export const runRemoteUpdate = async (): Promise<void> => {
  if (!canUpdate()) {
    postUpdateStatus({ ...INITIAL_STATE, status: 'error', error: 'updates disabled in dev builds' });
    return;
  }
  await checkForUpdate();
  if (getState().status !== 'available') return;
  await downloadAndInstall();
  if (getState().status !== 'ready') return;
  await restart();
};

export const useRemoteUpdate = (): void => {
  React.useEffect(() => {
    void reportBootStatus();
    return watchAppUpdateRequests(runRemoteUpdate);
  }, []);
};

export const useUpdater = () => {
  const current = React.useSyncExternalStore(subscribe, getState);

  React.useEffect(scheduleBootCheck, []);

  return { state: current, restart, dismiss, checkForUpdate, downloadAndInstall };
};
