import React from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { X, Copy, Minus, Square, Settings } from 'lucide-react';

import TabsBar from '~/components/commons/TabsBar';
import WorkspaceBar from '~/components/commons/WorkspaceBar';

import styles from './styles.module.scss';

const win = getCurrentWindow();

const Titlebar = () => {
  const [maximized, setMaximized] = React.useState(false);

  React.useEffect(() => {
    let unlisten: (() => void) | undefined;
    const sync = async () => setMaximized(await win.isMaximized());
    void sync();
    void win.onResized(sync).then((fn) => (unlisten = fn));
    return () => unlisten?.();
  }, []);

  const minimize = () => void win.minimize();
  const toggleMaximize = () => void win.toggleMaximize();
  const close = () => void win.close();

  const onDragStart = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest('button, [data-no-drag]')) return;
    if (e.detail === 2) {
      void win.toggleMaximize();
      return;
    }

    const cleanup = () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', cleanup);
    };
    const onMove = () => {
      cleanup();
      void win.startDragging();
    };

    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', cleanup);
  };

  return (
    <div className={styles.bar} onMouseDown={onDragStart}>
      <button className={styles.settings} aria-label="Settings">
        <Settings size={15} strokeWidth={1.75} />
      </button>
      <WorkspaceBar />
      <TabsBar />
      <div className={styles.controls}>
        <button className={styles.btn} onClick={minimize} aria-label="Minimize">
          <Minus size={16} strokeWidth={1.5} />
        </button>
        <button className={styles.btn} onClick={toggleMaximize} aria-label={maximized ? 'Restore' : 'Maximize'}>
          {maximized ? <Copy size={13} strokeWidth={1.5} /> : <Square size={12} strokeWidth={1.5} />}
        </button>
        <button className={`${styles.btn} ${styles.close}`} onClick={close} aria-label="Close">
          <X size={16} strokeWidth={1.5} />
        </button>
      </div>
    </div>
  );
};

export default Titlebar;
