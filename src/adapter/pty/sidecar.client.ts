const SIDECAR_HTTP = 'http://127.0.0.1:9777';

export const killPtySession = (tileId: string): Promise<unknown> =>
  fetch(`${SIDECAR_HTTP}/kill?tileId=${encodeURIComponent(tileId)}`).catch(() => undefined);

export const submitPtyMessage = async (tileId: string, text: string): Promise<boolean> => {
  try {
    const response = await fetch(`${SIDECAR_HTTP}/agent/send?tileId=${encodeURIComponent(tileId)}`, {
      method: 'POST',
      body: text
    });
    if (!response.ok) return false;
    const result = (await response.json()) as { ok?: boolean };
    return result.ok === true;
  } catch {
    return false;
  }
};
