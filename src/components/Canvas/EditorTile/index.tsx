import React from 'react';

import type { Tile } from '~/domain/interfaces/canvas.interface';
import FileEditor from '~/components/FileEditor';

import styles from './styles.module.scss';

interface EditorTileProps {
  tile: Tile;
  active: boolean;
  onActivate: (id: string) => void;
}

const EditorTile = ({ tile, active, onActivate }: EditorTileProps) => {
  const path = tile.filePath ?? '';

  const activate = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    onActivate(tile.id);
  };

  const stopWheel = (e: React.WheelEvent) => {
    if (active) e.stopPropagation();
  };

  return (
    <div className={styles.tile} onWheel={stopWheel} onPointerDown={activate}>
      <div className={active ? styles.host : `${styles.host} ${styles.inert}`}>
        <FileEditor path={path} active={active} />
      </div>
    </div>
  );
};

export default EditorTile;
