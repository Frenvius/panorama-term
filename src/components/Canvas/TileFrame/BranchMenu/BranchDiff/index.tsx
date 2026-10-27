import type { FileChange } from '~/domain/interfaces/git.interface';

import React from 'react';

import Dialog from '~/components/commons/Dialog';
import FileIcon from '~/components/commons/FileIcon';
import { requestRefDiff } from '~/usecase/util/refDiff';
import { gitDiffWithRef } from '~/adapter/git/git.client';
import { statusKey, STATUS_COLOR } from '~/usecase/util/fileTree';

import styles from './styles.module.scss';

interface BranchDiffProps {
  root: string;
  branch: string;
  onOpened: () => void;
  onClose: () => void;
}

const message = (err: unknown): string => (typeof err === 'string' ? err : String(err));

const label = (file: FileChange): string => (file.rename_from ? `${file.name} (from ${file.rename_from})` : file.name);

const BranchDiff = ({ root, branch, onOpened, onClose }: BranchDiffProps) => {
  const [files, setFiles] = React.useState<FileChange[] | null>(null);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let alive = true;
    gitDiffWithRef(root, branch)
      .then((list) => alive && setFiles(list))
      .catch((err: unknown) => alive && setError(message(err)));
    return () => {
      alive = false;
    };
  }, [root, branch]);

  const open = (path: string) => {
    requestRefDiff({ root, path, base: branch, files: (files ?? []).map((f) => f.path) });
    onOpened();
  };

  const count = files ? `${files.length} file${files.length === 1 ? '' : 's'} differ` : '';

  const footer = (
    <>
      <span className={styles.count}>{count}</span>
      <button type="button" className={styles.btn} onClick={onClose}>
        Close
      </button>
    </>
  );

  return (
    <Dialog title={`'${branch}' vs working tree`} footer={footer} onClose={onClose}>
      <div className={styles.list}>
        {!files && !error && <span className={styles.hint}>Loading...</span>}
        {files?.length === 0 && <span className={styles.hint}>No differences with the working tree.</span>}
        {files?.map((file) => {
          const key = statusKey(file);
          const select = () => open(file.path);
          return (
            <button key={file.path} type="button" className={styles.row} onClick={select} title={file.path}>
              <FileIcon name={file.name} size={14} />
              <span
                className={styles.name}
                style={{ color: STATUS_COLOR[key], textDecoration: key === 'deleted' ? 'line-through' : undefined }}
              >
                {label(file)}
              </span>
              {file.dir && <span className={styles.dir}>{file.dir}</span>}
            </button>
          );
        })}
      </div>
      {error && <p className={styles.error}>{error}</p>}
    </Dialog>
  );
};

export default BranchDiff;
