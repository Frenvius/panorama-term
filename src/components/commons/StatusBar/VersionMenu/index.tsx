import type { ContextMenuEntry } from '~/components/commons/ContextMenu';
import type { UpdaterState } from '~/usecase/hooks/useUpdater';

import React from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { Check, Download, RotateCw, RefreshCw } from 'lucide-react';

import ContextMenu from '~/components/commons/ContextMenu';
import { canUpdate, useUpdater } from '~/usecase/hooks/useUpdater';

import styles from './styles.module.scss';

const UP_TO_DATE_MS = 3000;

const ICON = { size: 14, strokeWidth: 1.75 };

const VersionMenu = () => {
  const { state, restart, checkForUpdate, downloadAndInstall } = useUpdater();
  const [version, setVersion] = React.useState<string | null>(null);
  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);
  const [upToDate, setUpToDate] = React.useState(false);
  const manualCheckRef = React.useRef(false);

  const { status } = state;

  React.useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch((err) => console.error('[version] getVersion failed', err));
  }, []);

  React.useEffect(() => {
    if (status === 'checking' || !manualCheckRef.current) return;
    manualCheckRef.current = false;
    if (status !== 'up-to-date') return;
    setUpToDate(true);
    const timer = window.setTimeout(() => setUpToDate(false), UP_TO_DATE_MS);
    return () => window.clearTimeout(timer);
  }, [status]);

  const closeMenu = React.useCallback(() => setMenu(null), []);

  const toggleMenu = (e: React.PointerEvent<HTMLButtonElement>) => {
    if (menu) {
      setMenu(null);
      return;
    }
    const rect = e.currentTarget.getBoundingClientRect();
    setMenu({ x: rect.right, y: rect.top - 4 });
  };

  const runManualCheck = () => {
    manualCheckRef.current = true;
    setUpToDate(false);
    void checkForUpdate();
  };

  if (!version) return null;

  const items = menuItems(state, { restart, runManualCheck, downloadAndInstall });
  const checking = status === 'checking';

  return (
    <>
      <button onPointerDown={toggleMenu} className={styles.trigger}>
        {checking && <RefreshCw size={11} strokeWidth={1.75} className={styles.spin} />}
        {upToDate && <Check size={11} strokeWidth={2} />}
        {upToDate ? 'Up to date' : `v${version}`}
      </button>
      {menu && <ContextMenu x={menu.x} y={menu.y} items={items} onClose={closeMenu} placement="above" />}
    </>
  );
};

interface MenuActions {
  restart: () => Promise<void>;
  runManualCheck: () => void;
  downloadAndInstall: () => Promise<void>;
}

const menuItems = (state: UpdaterState, actions: MenuActions): ContextMenuEntry[] => {
  if (state.status === 'available') {
    const install = () => void actions.downloadAndInstall();
    return [{ label: `Install v${state.version}`, icon: <Download {...ICON} />, onSelect: install }];
  }
  if (state.status === 'ready') {
    const restart = () => void actions.restart();
    return [{ label: 'Restart to update', icon: <RotateCw {...ICON} />, onSelect: restart }];
  }
  if (state.status === 'downloading') {
    return [{ label: 'Downloading update...', icon: <RefreshCw {...ICON} />, disabled: true }];
  }
  return [
    {
      icon: <RefreshCw {...ICON} />,
      onSelect: actions.runManualCheck,
      disabled: !canUpdate() || state.status === 'checking',
      label: canUpdate() ? 'Check for updates' : 'Updates disabled in dev'
    }
  ];
};

export default VersionMenu;
