import React from 'react';

import { readFileTail } from '~/adapter/fs/fs.client';

const POLL_MS = 1000;
const TAIL_BYTES = 64 * 1024;
const ANSI_RE = /\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;
const BARE_CR_RE = /\r(?!\n)/g;

// `live` false reads once: the file no longer grows.
export const useFileTail = (path: string | undefined, { maxBytes = TAIL_BYTES, live = true }: { maxBytes?: number; live?: boolean } = {}) => {
  const [text, setText] = React.useState('');
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (!path) return;
    setText('');
    setError(null);
    let alive = true;
    const load = () => {
      readFileTail(path, maxBytes)
        .then((raw) => {
          if (!alive) return;
          setText(raw.replace(ANSI_RE, '').replace(BARE_CR_RE, '\n'));
          setError(null);
        })
        .catch((e) => alive && setError(String(e)));
    };
    load();
    const timer = live ? window.setInterval(load, POLL_MS) : undefined;
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [path, maxBytes, live]);

  return { text, error };
};
