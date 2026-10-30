import type { MobileState, MobilePairing } from '~/domain/interfaces/mobile.interface';

import React from 'react';

import {
  setMobileRemote,
  fetchMobileState,
  setMobileEnabled,
  startMobilePairing,
  revokeMobileDevice,
  cancelMobilePairing
} from '~/adapter/mobile/mobile.client';

const PAIRING_POLL_MS = 2000;
const RELAY_POLL_MS = 3000;

export const useMobileAccess = () => {
  const [state, setState] = React.useState<MobileState | null>(null);
  const [pairing, setPairing] = React.useState<MobilePairing | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  const track = React.useCallback(<T,>(task: Promise<T>, apply: (value: T) => void) => {
    task
      .then((value) => {
        setError(null);
        apply(value);
      })
      .catch((e: unknown) => setError(String(e)));
  }, []);

  React.useEffect(() => {
    track(fetchMobileState(), setState);
  }, [track]);

  const deviceCount = state?.devices.length ?? 0;

  React.useEffect(() => {
    if (!pairing) return;
    const timer = window.setInterval(() => {
      if (Date.now() > pairing.expiresAt) {
        setPairing(null);
        return;
      }
      track(fetchMobileState(), (next) => {
        setState(next);
        if (next.devices.length > deviceCount) setPairing(null);
      });
    }, PAIRING_POLL_MS);
    return () => window.clearInterval(timer);
  }, [pairing, deviceCount, track]);

  const relayPending = !!state?.enabled && state.remote && deviceCount > 0 && state.relay !== 'online';

  React.useEffect(() => {
    if (!relayPending) return;
    const timer = window.setInterval(() => track(fetchMobileState(), setState), RELAY_POLL_MS);
    return () => window.clearInterval(timer);
  }, [relayPending, track]);

  const toggleRemote = () => {
    if (!state) return;
    track(setMobileRemote(!state.remote), setState);
  };

  const toggleEnabled = () => {
    if (!state) return;
    if (state.enabled) setPairing(null);
    track(setMobileEnabled(!state.enabled), setState);
  };

  const pair = () => {
    track(startMobilePairing(), (next) => {
      setPairing(next);
      track(fetchMobileState(), setState);
    });
  };

  const cancelPairing = () => {
    setPairing(null);
    track(cancelMobilePairing(), setState);
  };

  const revoke = (id: string) => track(revokeMobileDevice(id), setState);

  return { state, pairing, error, toggleEnabled, toggleRemote, pair, cancelPairing, revoke };
};
