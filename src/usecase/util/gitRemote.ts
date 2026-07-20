const SCP = /^[\w.-]+@([^:/]+):(.+)$/;
const WEB_PROTOCOL = /^(https?|ssh|git):$/;

interface RemoteParts {
  host: string;
  repo: string;
}

const parseRemote = (remote: string): RemoteParts | null => {
  const trimmed = remote.trim();
  if (!trimmed) return null;

  const scp = SCP.exec(trimmed);
  const parts = ((): RemoteParts | null => {
    if (scp) return { host: scp[1], repo: scp[2] };
    try {
      const url = new URL(trimmed);
      if (!WEB_PROTOCOL.test(url.protocol)) return null;
      return { host: url.hostname, repo: url.pathname };
    } catch {
      return null;
    }
  })();

  if (!parts) return null;

  const repo = parts.repo.replace(/^\/+/, '').replace(/\/+$/, '').replace(/\.git$/, '');
  if (!parts.host || !repo) return null;
  return { host: parts.host, repo };
};

const commitSegment = (host: string): string => {
  if (host.includes('gitlab')) return '-/commit';
  if (host.includes('bitbucket')) return 'commits';
  return 'commit';
};

export const commitUrl = (remote: string, hash: string): string | null => {
  const parts = parseRemote(remote);
  if (!parts || !/^[0-9a-f]{4,40}$/i.test(hash)) return null;
  return `https://${parts.host}/${parts.repo}/${commitSegment(parts.host)}/${hash}`;
};
