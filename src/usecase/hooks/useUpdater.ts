import React from 'react';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';

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
const listeners = new Set<() => void>();

const setState = (patch: Partial<UpdaterState>) => {
  state = { ...state, ...patch };
  listeners.forEach((fn) => fn());
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

export const useUpdater = () => {
  const current = React.useSyncExternalStore(subscribe, getState);

  React.useEffect(scheduleBootCheck, []);

  return { state: current, restart, dismiss, checkForUpdate, downloadAndInstall };
};
