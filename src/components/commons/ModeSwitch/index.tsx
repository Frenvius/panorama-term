import type { TabMode } from '~/domain/interfaces/projectView.interface';

import { Columns3, LayoutGrid } from 'lucide-react';

import styles from './styles.module.scss';
import { formatCombo, getBinding } from '~/usecase/util/keybindings';
import { useWorkspace } from '~/usecase/context/WorkspaceContext';

const ModeSwitch = () => {
  const { activeTabId, activeTabMode, setTabMode } = useWorkspace();

  if (!activeTabId) return null;

  const selectMode = (mode: TabMode) => {
    if (mode !== activeTabMode) void setTabMode(activeTabId, mode);
  };
  const selectCanvas = () => selectMode('canvas');
  const selectProject = () => selectMode('project');
  const shortcut = formatCombo(getBinding('view.toggleMode'));

  return (
    <div className={styles.switch} role="radiogroup" aria-label="Tab mode" data-no-drag>
      <button
        type="button"
        role="radio"
        className={styles.option}
        onClick={selectCanvas}
        data-shortcut={shortcut}
        data-tooltip-place="bottom"
        data-tooltip="Canvas mode"
        aria-checked={activeTabMode === 'canvas'}
      >
        <LayoutGrid size={13} strokeWidth={1.75} />
        <span>Canvas</span>
      </button>
      <button
        type="button"
        role="radio"
        className={styles.option}
        onClick={selectProject}
        data-shortcut={shortcut}
        data-tooltip-place="bottom"
        data-tooltip="Project mode"
        aria-checked={activeTabMode === 'project'}
      >
        <Columns3 size={13} strokeWidth={1.75} />
        <span>Project</span>
      </button>
    </div>
  );
};

export default ModeSwitch;
