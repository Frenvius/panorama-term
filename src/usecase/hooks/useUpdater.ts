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

const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const useUpdater = () => {
  const [state, setState] = React.useState<UpdaterState>(INITIAL_STATE);
  const updateRef = React.useRef<Update | null>(null);
  const checkingRef = React.useRef(false);

  const checkForUpdate = React.useCallback(async () => {
    if (!isTauri() || checkingRef.current) return;
    checkingRef.current = true;
    setState((prev) => ({ ...prev, error: null, status: 'checking' }));

    try {
      const result = await check();
      if (!result) {
        updateRef.current = null;
        setState((prev) => ({ ...prev, status: 'up-to-date' }));
        return;
      }
      updateRef.current = result;
      setState((prev) => ({
        ...prev,
        status: 'available',
        version: result.version,
        notes: result.body ?? null,
        currentVersion: result.currentVersion,
      }));
    } catch (err) {
      setState((prev) => ({ ...prev, status: 'error', error: err instanceof Error ? err.message : String(err) }));
    } finally {
      checkingRef.current = false;
    }
  }, []);

  const downloadAndInstall = React.useCallback(async () => {
    const update = updateRef.current;
    if (!update) return;
    setState((prev) => ({ ...prev, error: null, downloaded: 0, contentLength: 0, status: 'downloading' }));

    try {
      let downloaded = 0;
      await update.downloadAndInstall((event) => {
        switch (event.event) {
          case 'Started':
            setState((prev) => ({ ...prev, contentLength: event.data.contentLength ?? 0, downloaded: 0 }));
            break;
          case 'Progress':
            downloaded += event.data.chunkLength;
            setState((prev) => ({ ...prev, downloaded }));
            break;
          case 'Finished':
            setState((prev) => ({ ...prev, status: 'ready' }));
            break;
        }
      });
    } catch (err) {
      setState((prev) => ({ ...prev, status: 'error', error: err instanceof Error ? err.message : String(err) }));
    }
  }, []);

  const restart = React.useCallback(async () => {
    try {
      await relaunch();
    } catch (err) {
      setState((prev) => ({ ...prev, status: 'error', error: err instanceof Error ? err.message : String(err) }));
    }
  }, []);

  const dismiss = React.useCallback(() => {
    updateRef.current = null;
    setState(INITIAL_STATE);
  }, []);

  React.useEffect(() => {
    if (!isTauri()) return;
    const timer = window.setTimeout(() => void checkForUpdate(), 3000);
    return () => window.clearTimeout(timer);
  }, [checkForUpdate]);

  return { state, restart, dismiss, checkForUpdate, downloadAndInstall };
};
