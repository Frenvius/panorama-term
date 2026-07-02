import React from 'react';

import styles from './styles.module.scss';

export interface ContextMenuItem {
  label: string;
  onSelect: () => void;
  icon?: React.ReactNode;
}

interface ContextMenuProps {
  x: number;
  y: number;
  onClose: () => void;
  items: ContextMenuItem[];
}

const ContextMenu = ({ x, y, items, onClose }: ContextMenuProps) => {
  const rootRef = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    const onOutside = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('pointerdown', onOutside, true);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('pointerdown', onOutside, true);
      document.removeEventListener('keydown', onKey, true);
    };
  }, [onClose]);

  return (
    <div ref={rootRef} className={styles.menu} style={{ top: y, left: x }}>
      {items.map((item) => {
        const select = () => {
          item.onSelect();
          onClose();
        };

        return (
          <button key={item.label} className={styles.item} onClick={select}>
            {item.icon}
            <span>{item.label}</span>
          </button>
        );
      })}
    </div>
  );
};

export default ContextMenu;
