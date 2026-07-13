import React from 'react';
import { Download, RotateCw, RefreshCw, AlertCircle } from 'lucide-react';

import { useUpdater } from '~/usecase/hooks/useUpdater';

import styles from './styles.module.scss';

const formatBytes = (bytes: number) => {
  if (!bytes) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB'];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  return `${(bytes / Math.pow(1024, i)).toFixed(i === 0 ? 0 : 1)} ${units[i]}`;
};

const UpdateIndicator = () => {
  const { state, restart, dismiss, checkForUpdate, downloadAndInstall } = useUpdater();
  const [open, setOpen] = React.useState(false);
  const rootRef = React.useRef<HTMLDivElement | null>(null);

  const { notes, error, status, version, downloaded, contentLength, currentVersion } = state;

  React.useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener('mousedown', onDown);
    return () => window.removeEventListener('mousedown', onDown);
  }, [open]);

  if (status === 'idle' || status === 'checking' || status === 'up-to-date') return null;

  const progressPct = contentLength > 0 ? Math.min(100, (downloaded / contentLength) * 100) : 0;

  const toggleOpen = () => setOpen((prev) => !prev);
  const onInstall = () => void downloadAndInstall();
  const onRestart = () => void restart();
  const onRetry = () => void checkForUpdate();
  const onDismiss = () => {
    dismiss();
    setOpen(false);
  };

  const title =
    status === 'error'
      ? `Update error: ${error ?? 'unknown'}`
      : status === 'ready'
        ? 'Update ready: restart to apply'
        : status === 'downloading'
          ? `Downloading update ${Math.round(progressPct)}%`
          : `Update available: v${version}`;

  return (
    <div className={styles.root} ref={rootRef} data-no-drag>
      <button className={`${styles.trigger} ${status === 'error' ? styles.err : ''}`} onClick={toggleOpen} title={title}>
        {status === 'error' && <AlertCircle size={14} strokeWidth={1.75} />}
        {status === 'ready' && <RotateCw size={14} strokeWidth={1.75} />}
        {status === 'downloading' && <RefreshCw size={14} strokeWidth={1.75} className={styles.spin} />}
        {status === 'available' && (
          <>
            <Download size={14} strokeWidth={1.75} />
            <span className={styles.dot} />
          </>
        )}
      </button>
      {open && (
        <div className={styles.panel}>
          <div className={styles.header}>
            <span className={styles.heading}>
              {status === 'available' && 'Update available'}
              {status === 'downloading' && 'Downloading update'}
              {status === 'ready' && 'Restart required'}
              {status === 'error' && 'Update failed'}
            </span>
            {version && <span className={styles.version}>v{version}</span>}
          </div>
          {currentVersion && <div className={styles.current}>Current: v{currentVersion}</div>}
          {status === 'available' && (
            <>
              {notes && <pre className={styles.notes}>{notes}</pre>}
              <div className={styles.actions}>
                <button className={styles.primary} onClick={onInstall}>
                  Download & Install
                </button>
                <button className={styles.ghost} onClick={onDismiss}>
                  Later
                </button>
              </div>
            </>
          )}
          {status === 'downloading' && (
            <>
              <div className={styles.track}>
                <div className={styles.fill} style={{ width: `${progressPct}%` }} />
              </div>
              <div className={styles.bytes}>
                <span>
                  {formatBytes(downloaded)}
                  {contentLength > 0 && ` / ${formatBytes(contentLength)}`}
                </span>
                <span>{Math.round(progressPct)}%</span>
              </div>
            </>
          )}
          {status === 'ready' && (
            <>
              <div className={styles.text}>Update installed. Restart Panorama to start using the new version.</div>
              <div className={styles.actions}>
                <button className={styles.primary} onClick={onRestart}>
                  Restart now
                </button>
                <button className={styles.ghost} onClick={toggleOpen}>
                  Later
                </button>
              </div>
            </>
          )}
          {status === 'error' && (
            <>
              <div className={styles.errorText}>{error ?? 'Could not check for updates.'}</div>
              <div className={styles.actions}>
                <button className={styles.primary} onClick={onRetry}>
                  Try again
                </button>
                <button className={styles.ghost} onClick={onDismiss}>
                  Dismiss
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
};

export default UpdateIndicator;
