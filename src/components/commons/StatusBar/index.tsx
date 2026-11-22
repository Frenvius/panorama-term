import type { ContextMenuEntry } from '~/components/commons/ContextMenu';

import React from 'react';
import { listen } from '@tauri-apps/api/event';
import { Plus, Check, RefreshCw } from 'lucide-react';

import ContextMenu from '~/components/commons/ContextMenu';
import { useAgentUsage } from '~/usecase/hooks/useAgentUsage';
import { useUsageProviders, toggleUsageProvider } from '~/usecase/hooks/useUsageProviders';
import VersionMenu from '~/components/commons/StatusBar/VersionMenu';
import UsageSegment from '~/components/commons/StatusBar/UsageSegment';
import { PROVIDER_NAMES } from '~/components/commons/StatusBar/constants';
import { USAGE_PROVIDERS } from '~/usecase/util/constants';
import { useRemoteStatus } from '~/usecase/hooks/useRemoteStatus';
import { remoteLabel, remoteTransferLabel, type RemoteTransfer } from '~/usecase/util/remote';
import { requestSettings } from '~/usecase/util/settingsNav';

import styles from './styles.module.scss';

const StatusBar = () => {
  const enabled = useUsageProviders();
  const [menu, setMenu] = React.useState<{ x: number; y: number } | null>(null);
  const { usage, loading, refresh } = useAgentUsage(enabled);
  const remote = useRemoteStatus();
  const [transfer, setTransfer] = React.useState<RemoteTransfer | null>(null);
  React.useEffect(() => {
    let active = true;
    const subscription = listen<RemoteTransfer | { done: true }>('remote-transfer', ({ payload }) => {
      if (active) setTransfer(payload.done === true ? null : payload as RemoteTransfer);
    });
    return () => { active = false; void subscription.then((unlisten) => unlisten()).catch(() => {}); };
  }, []);

  const openMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY });
  };

  const closeMenu = React.useCallback(() => setMenu(null), []);

  const items: ContextMenuEntry[] = USAGE_PROVIDERS.map((provider) => ({
    onSelect: () => toggleUsageProvider(provider),
    label: `${PROVIDER_NAMES[provider]} Usage`,
    icon: enabled.includes(provider) ? <Check size={14} strokeWidth={2} /> : <span />
  }));

  const shown = usage.filter((u) => enabled.includes(u.provider) && u.error !== 'not-installed');

  return (
    <footer className={styles.root} onContextMenu={openMenu}>
      {remote && remote.status !== 'disconnected' && (
        <button className={styles.add} onClick={() => requestSettings('remote')}>
          {remoteLabel(remote)}
          {remote.transport === 'relay' && transfer && (
            <span className={styles.transfer}>
              {remoteTransferLabel(transfer)}
              {transfer.total != null && transfer.total > 0 && <span className={styles.progress}><span style={{ width: `${Math.min(100, transfer.done / transfer.total * 100)}%` }} /></span>}
            </span>
          )}
        </button>
      )}
      {shown.map((u) => (
        <UsageSegment usage={u} key={u.provider} />
      ))}
      {enabled.length > 0 ? (
        <button onClick={refresh} className={styles.icon} data-tooltip-place="top" data-tooltip="Refresh usage">
          <RefreshCw size={11} strokeWidth={1.75} className={loading ? styles.spin : undefined} />
        </button>
      ) : (
        <button onClick={openMenu} className={styles.add}>
          <Plus size={11} strokeWidth={1.75} />
          Usage
        </button>
      )}
      {menu && <ContextMenu x={menu.x} y={menu.y} items={items} onClose={closeMenu} />}
      <VersionMenu />
    </footer>
  );
};

export default StatusBar;
