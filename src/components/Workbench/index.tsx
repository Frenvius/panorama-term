import React from 'react';
import { X, Save, Search, GitCompare, LayoutGrid } from 'lucide-react';

import type { EditorTab } from '~/domain/interfaces/editor.interface';
import DiffViewer from '~/components/DiffViewer';
import FileEditor from '~/components/FileEditor';
import FileIcon from '~/components/commons/FileIcon';
import { fileName } from '~/usecase/util/codeEditor';
import UnsavedDialog from '~/components/commons/UnsavedDialog';
import { getBinding, formatCombo } from '~/usecase/util/keybindings';
import { isDirty, requestSave, requestFind, subscribeDirty } from '~/usecase/util/dirtyFiles';

import styles from './styles.module.scss';

export interface WorkbenchHandlers {
  onSelect: (key: string) => void;
  onCloseTab: (key: string) => void;
  onAddToCanvas: (tab: EditorTab) => void;
  onStepFile: (step: number) => void;
  onPinTab: (key: string) => void;
  onClose: () => void;
}

interface WorkbenchProps {
  tabs: EditorTab[];
  active: string;
  exiting?: boolean;
  diffFiles: string[];
  handlers: WorkbenchHandlers;
}

const filePaths = (tabs: EditorTab[]): string[] => tabs.filter((t) => t.kind === 'file').map((t) => t.path);

const Workbench = ({ tabs, active, exiting, diffFiles, handlers }: WorkbenchProps) => {
  const { onSelect, onCloseTab, onAddToCanvas, onStepFile, onPinTab, onClose } = handlers;
  const [pending, setPending] = React.useState<{ keys: string[]; scope: 'tab' | 'all' } | null>(null);

  const dirtyKey = React.useSyncExternalStore(subscribeDirty, () => filePaths(tabs).filter(isDirty).join('\n'));
  const dirtyPaths = React.useMemo(() => (dirtyKey ? dirtyKey.split('\n') : []), [dirtyKey]);
  const dirtyKeys = React.useMemo(
    () => tabs.filter((t) => t.kind === 'file' && dirtyPaths.includes(t.path)).map((t) => t.key),
    [tabs, dirtyPaths]
  );

  const activeTab = tabs.find((t) => t.key === active) ?? null;
  const activeDirty = activeTab ? dirtyKeys.includes(activeTab.key) : false;

  const requestCloseAll = React.useCallback(() => {
    if (!dirtyKeys.length) {
      onClose();
      return;
    }
    setPending({ keys: dirtyKeys, scope: 'all' });
  }, [dirtyKeys, onClose]);

  React.useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape' || pending) return;
      e.stopPropagation();
      requestCloseAll();
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [pending, requestCloseAll]);

  const closeTab = (tab: EditorTab) => {
    if (dirtyKeys.includes(tab.key)) {
      setPending({ keys: [tab.key], scope: 'tab' });
      return;
    }
    onCloseTab(tab.key);
  };

  const discardPending = () => {
    if (!pending) return;
    setPending(null);
    if (pending.scope === 'all') {
      onClose();
      return;
    }
    onCloseTab(pending.keys[0]);
  };

  const savePending = async () => {
    const targets = tabs.filter((t) => pending?.keys.includes(t.key));
    for (const tab of targets) await requestSave(tab.path);
    discardPending();
  };

  const pendingPaths = tabs.filter((t) => pending?.keys.includes(t.key)).map((t) => t.path);

  const onSavePending = () => void savePending();
  const cancelPending = () => setPending(null);
  const saveActive = () => activeTab && void requestSave(activeTab.path);
  const findActive = () => requestFind();
  const addToCanvas = () => activeTab && onAddToCanvas(activeTab);

  const diffAt = activeTab?.kind === 'diff' ? diffFiles.indexOf(activeTab.path) : -1;
  const prevFile = diffAt > 0 ? () => onStepFile(-1) : undefined;
  const nextFile = diffAt >= 0 && diffAt < diffFiles.length - 1 ? () => onStepFile(1) : undefined;

  const shell = exiting ? `${styles.overlay} ${styles.exit}` : styles.overlay;

  return (
    <div className={shell}>
      <div className={styles.header}>
        <div className={styles.tabs}>
          {tabs.map((tab) => {
            const select = () => onSelect(tab.key);
            const pin = () => onPinTab(tab.key);
            const close = (e: React.MouseEvent) => {
              e.stopPropagation();
              closeTab(tab);
            };

            return (
              <div
                key={tab.key}
                className={styles.tab}
                onClick={select}
                onDoubleClick={pin}
                title={tab.path}
                data-active={tab.key === active || undefined}
                data-preview={tab.preview || undefined}
              >
                {tab.kind === 'diff' ? (
                  <GitCompare size={13} strokeWidth={2} className={styles.diffIcon} />
                ) : (
                  <FileIcon name={fileName(tab.path)} size={13} />
                )}
                <span className={styles.name}>{fileName(tab.path)}</span>
                {dirtyKeys.includes(tab.key) && <span className={styles.dot} />}
                <button className={styles.tabClose} onClick={close} aria-label="Close tab">
                  <X size={12} strokeWidth={2} />
                </button>
              </div>
            );
          })}
        </div>
        <span className={styles.spacer} />
        {activeTab?.kind === 'file' && (
          <>
            <button
              className={styles.action}
              onClick={saveActive}
              disabled={!activeDirty}
              data-tooltip="Save"
              data-shortcut={formatCombo(getBinding('editor.save'))}
              aria-label="Save"
            >
              <Save size={14} strokeWidth={2} />
            </button>
            <button
              className={styles.action}
              onClick={findActive}
              data-tooltip="Find"
              data-shortcut={formatCombo(getBinding('editor.find'))}
              aria-label="Find"
            >
              <Search size={14} strokeWidth={2} />
            </button>
          </>
        )}
        <button
          className={styles.action}
          onClick={addToCanvas}
          disabled={!activeTab || activeDirty}
          data-tooltip={activeDirty ? 'Save before adding to canvas' : 'Add to canvas'}
          aria-label="Add to canvas"
        >
          <LayoutGrid size={14} strokeWidth={2} />
        </button>
        <button className={styles.action} onClick={requestCloseAll} data-tooltip="Close" aria-label="Close">
          <X size={15} strokeWidth={2} />
        </button>
      </div>

      <div className={styles.body}>
        {tabs.map((tab) => {
          if (tab.kind === 'file') {
            return (
              <div key={tab.key} className={styles.pane} data-hidden={tab.key !== active || undefined}>
                <FileEditor path={tab.path} active={tab.key === active} />
              </div>
            );
          }
          if (tab.key !== active) return null;

          return (
            <div key={tab.key} className={styles.pane}>
              <DiffViewer
                root={tab.root}
                file={tab.path}
                commit={tab.commit}
                mode={{ embedded: true, keys: true }}
                handlers={{ onPrevFile: prevFile, onNextFile: nextFile }}
              />
            </div>
          );
        })}
      </div>

      {pending && (
        <UnsavedDialog
          paths={pendingPaths}
          onSave={onSavePending}
          onDiscard={discardPending}
          onCancel={cancelPending}
        />
      )}
    </div>
  );
};

export default Workbench;
