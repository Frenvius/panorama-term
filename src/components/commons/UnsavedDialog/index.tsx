import Dialog from '~/components/commons/Dialog';
import { fileName } from '~/usecase/util/codeEditor';

import styles from './styles.module.scss';

interface UnsavedDialogProps {
  paths: string[];
  onSave: () => void;
  onDiscard: () => void;
  onCancel: () => void;
}

const UnsavedDialog = ({ paths, onSave, onDiscard, onCancel }: UnsavedDialogProps) => {
  const footer = (
    <>
      <button type="button" className={styles.btn} onClick={onCancel}>
        Cancel
      </button>
      <button type="button" className={`${styles.btn} ${styles.danger}`} onClick={onDiscard}>
        Discard
      </button>
      <button type="submit" className={`${styles.btn} ${styles.primary}`}>
        Save
      </button>
    </>
  );

  return (
    <Dialog title="Unsaved changes" footer={footer} onClose={onCancel} onSubmit={onSave}>
      <p className={styles.message}>
        {paths.length === 1
          ? `${fileName(paths[0])} has unsaved changes.`
          : `${paths.length} files have unsaved changes.`}
      </p>
      {paths.length > 1 && (
        <ul className={styles.list}>
          {paths.map((path) => (
            <li key={path}>{fileName(path)}</li>
          ))}
        </ul>
      )}
    </Dialog>
  );
};

export default UnsavedDialog;
