import React from 'react';
import { Play, Power, Square, RotateCw, RefreshCw, LoaderCircle } from 'lucide-react';

import type { DockerAction, DockerContainer } from '~/domain/interfaces/docker.interface';
import { dockerPs, dockerAction, dockerEngine } from '~/adapter/docker/docker.client';
import { useWindowedList } from '~/usecase/hooks/useWindowedList';

import styles from './styles.module.scss';

interface DockerTabProps {
  query: string;
}

const POLL_MS = 3000;
const ROW_HEIGHT = 56;

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const DockerTab = ({ query }: DockerTabProps) => {
  const [containers, setContainers] = React.useState<DockerContainer[]>([]);
  const [error, setError] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState<Set<string>>(() => new Set());
  const [loading, setLoading] = React.useState(true);
  const [starting, setStarting] = React.useState(false);

  const inFlight = React.useRef(false);
  const lastKey = React.useRef('');

  const refresh = React.useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    try {
      const next = await dockerPs();
      const key = JSON.stringify(next);
      if (key !== lastKey.current) {
        lastKey.current = key;
        setContainers(next);
      }
      setError(null);
      setStarting(false);
    } catch (err) {
      setError(message(err));
    } finally {
      inFlight.current = false;
      setLoading(false);
    }
  }, []);

  React.useEffect(() => {
    let alive = true;
    const tick = () => {
      if (alive) void refresh();
    };
    tick();
    const timer = window.setInterval(tick, POLL_MS);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [refresh]);

  const run = async (id: string, action: DockerAction) => {
    setBusy((prev) => new Set(prev).add(id));
    try {
      await dockerAction(id, action);
    } catch (err) {
      setError(message(err));
    } finally {
      setBusy((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
      void refresh();
    }
  };

  const startEngine = async () => {
    setStarting(true);
    try {
      await dockerEngine('start');
    } catch (err) {
      setError(message(err));
      setStarting(false);
    }
  };

  const stopEngine = () => void dockerEngine('stop').catch((err) => setError(message(err)));

  const visible = containers.filter((c) => !query || c.name.toLowerCase().includes(query));
  const running = containers.filter((c) => c.state === 'running').length;

  const { attach, start: first, end, before, after } = useWindowedList({ count: visible.length, rowHeight: ROW_HEIGHT });

  if (loading) return <div className={styles.notice}>Loading containers</div>;

  if (error) {
    return (
      <div className={styles.notice} title={error}>
        {starting ? (
          <>
            <LoaderCircle size={16} strokeWidth={1.75} className={styles.spinning} />
            Starting Docker engine
          </>
        ) : (
          <>
            Docker engine is not running
            <button className={styles.engine} onClick={() => void startEngine()}>
              <Play size={13} strokeWidth={1.75} />
              Start Docker
            </button>
          </>
        )}
      </div>
    );
  }

  if (containers.length === 0) return <div className={styles.notice}>No containers</div>;

  return (
    <div className={styles.root}>
      <div className={styles.head}>
        <span>Containers</span>
        <span className={styles.count}>
          {running}/{containers.length}
        </span>
        <button className={styles.tool} onClick={() => void refresh()} aria-label="Refresh">
          <RefreshCw size={13} strokeWidth={1.75} />
        </button>
        <button className={styles.tool} onClick={stopEngine} aria-label="Stop Docker engine" data-danger>
          <Power size={13} strokeWidth={1.75} />
        </button>
      </div>
      <div ref={attach} className={styles.list}>
        <div style={{ height: before }} />
        {visible.slice(first, end).map((container) => {
          const up = container.state === 'running';
          const pending = busy.has(container.id);
          const start = () => void run(container.id, 'start');
          const stop = () => void run(container.id, 'stop');
          const restart = () => void run(container.id, 'restart');

          return (
            <div key={container.id} className={styles.row}>
              <span className={styles.dot} data-up={up || undefined} title={container.state} />
              <div className={styles.text}>
                <span className={styles.name}>{container.name || container.id.slice(0, 12)}</span>
                <span className={styles.sub} title={container.image}>
                  {container.image}
                </span>
                <span className={styles.status}>{container.status}</span>
              </div>
              <div className={styles.actions}>
                {up ? (
                  <>
                    <button className={styles.tool} onClick={restart} disabled={pending} aria-label="Restart">
                      <RotateCw size={13} strokeWidth={1.75} />
                    </button>
                    <button className={styles.tool} onClick={stop} disabled={pending} aria-label="Stop" data-danger>
                      <Square size={13} strokeWidth={1.75} />
                    </button>
                  </>
                ) : (
                  <button className={styles.tool} onClick={start} disabled={pending} aria-label="Start">
                    <Play size={13} strokeWidth={1.75} />
                  </button>
                )}
              </div>
            </div>
          );
        })}
        <div style={{ height: after }} />
      </div>
    </div>
  );
};

export default DockerTab;
