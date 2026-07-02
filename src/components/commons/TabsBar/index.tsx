import React from 'react';
import { X, Plus } from 'lucide-react';

import { useWorkspace } from '~/usecase/context/WorkspaceContext';

import styles from './styles.module.scss';

const TabsBar = () => {
  const { tabs, activeTabId, switchTab, createTab, deleteTab, renameTab } = useWorkspace();

  const addTab = () => void createTab();

  return (
    <div data-no-drag className={styles.bar}>
      {tabs.map((tab) => {
        const select = () => void switchTab(tab.id);
        const rename = () => {
          const name = window.prompt('Tab name', tab.name);
          if (name) void renameTab(tab.id, name);
        };
        const close = (e: React.MouseEvent) => {
          e.stopPropagation();
          void deleteTab(tab.id);
        };

        return (
          <div
            key={tab.id}
            className={styles.tab}
            onClick={select}
            onDoubleClick={rename}
            data-active={tab.id === activeTabId}
          >
            <span className={styles.name}>{tab.name}</span>
            <button
              className={styles.close}
              onClick={close}
              aria-label="Close tab"
              data-hidden={tabs.length <= 1}
            >
              <X size={13} strokeWidth={2} />
            </button>
          </div>
        );
      })}
      <button className={styles.add} onClick={addTab} aria-label="New tab">
        <Plus size={16} strokeWidth={2} />
      </button>
    </div>
  );
};

export default TabsBar;
