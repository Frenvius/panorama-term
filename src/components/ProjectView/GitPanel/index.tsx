import GitTab from '~/components/Canvas/Navigator/GitTab';
import CrashNotice from '~/components/commons/CrashNotice';
import ErrorBoundary from '~/components/commons/ErrorBoundary';
import styles from '~/components/ProjectView/panels.module.scss';

export interface GitPanelProps {
  activeDiff: string | null;
  root: string | null;
  onDiffFiles: (files: string[]) => void;
  onOpenDiff: (root: string, filePath: string, opts?: { sideSplit?: boolean }) => void;
  onOpenFile: (root: string, filePath: string, opts?: { preview?: boolean; sideSplit?: boolean }) => void;
}

const crashFallback = (retry: () => void) => (
  <CrashNotice title="Source control panel hit an error" action="Reload panel" onAction={retry} />
);

const GitPanel = ({ activeDiff, root, onDiffFiles, onOpenDiff, onOpenFile }: GitPanelProps) => {
  const handleOpenDiff = (repo: string, file: string) => onOpenDiff(repo, file);
  const handleOpenFile = (file: string) => {
    if (root) onOpenFile(root, file);
  };

  return root ? (
    <ErrorBoundary fallback={crashFallback}>
      <GitTab
        query=""
        active={activeDiff}
        key={root}
        root={root}
        onFiles={onDiffFiles}
        onOpenDiff={handleOpenDiff}
        onOpenFile={handleOpenFile}
      />
    </ErrorBoundary>
  ) : (
    <div className={styles.empty}>Select a project to view source control</div>
  );
};

export default GitPanel;
