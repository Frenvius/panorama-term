import React from 'react';
import ReactDOM from 'react-dom';
import { ChevronRight } from 'lucide-react';

import styles from './styles.module.scss';

export interface ContextMenuItem {
  label: string;
  onSelect?: () => void;
  icon?: React.ReactNode;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  submenu?: ContextMenuEntry[];
}

export type ContextMenuEntry = ContextMenuItem | 'separator';

interface ContextMenuProps {
  x: number;
  y: number;
  onClose: () => void;
  items: ContextMenuEntry[];
}

const EDGE = 8;

const ContextMenu = ({ x, y, items, onClose }: ContextMenuProps) => {
  const rootRef = React.useRef<HTMLDivElement>(null);
  const [pos, setPos] = React.useState({ x, y });

  React.useLayoutEffect(() => {
    const el = rootRef.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const nx = Math.min(x, window.innerWidth - width - EDGE);
    const ny = Math.min(y, window.innerHeight - height - EDGE);
    setPos({ x: Math.max(EDGE, nx), y: Math.max(EDGE, ny) });
  }, [x, y]);

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

  const stop = (e: React.PointerEvent) => e.stopPropagation();

  return ReactDOM.createPortal(
    <div ref={rootRef} className={styles.menu} style={{ top: pos.y, left: pos.x }} onPointerDown={stop}>
      <Entries items={items} onClose={onClose} />
    </div>,
    document.body
  );
};

interface EntriesProps {
  items: ContextMenuEntry[];
  onClose: () => void;
}

const Entries = ({ items, onClose }: EntriesProps) => (
  <>
    {items.map((item, i) => {
      if (item === 'separator') return <div key={`sep-${i}`} className={styles.separator} />;
      if (item.submenu) return <SubMenu key={item.label} item={item} onClose={onClose} />;

      const select = () => {
        if (item.disabled) return;
        item.onSelect?.();
        onClose();
      };

      const cls = [styles.item, item.danger && styles.danger, item.disabled && styles.disabled]
        .filter(Boolean)
        .join(' ');

      return (
        <button key={item.label} className={cls} onClick={select} disabled={item.disabled}>
          {item.icon && <span className={styles.icon}>{item.icon}</span>}
          <span className={styles.label}>{item.label}</span>
          {item.shortcut && <span className={styles.shortcut}>{item.shortcut}</span>}
        </button>
      );
    })}
  </>
);

interface SubMenuProps {
  item: ContextMenuItem;
  onClose: () => void;
}

const SubMenu = ({ item, onClose }: SubMenuProps) => {
  const [open, setOpen] = React.useState(false);
  const timer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const enter = () => {
    clearTimeout(timer.current);
    setOpen(true);
  };
  const leave = () => {
    timer.current = setTimeout(() => setOpen(false), 120);
  };

  React.useEffect(() => () => clearTimeout(timer.current), []);

  return (
    <div className={styles.subAnchor} onPointerEnter={enter} onPointerLeave={leave}>
      <button className={styles.item}>
        {item.icon && <span className={styles.icon}>{item.icon}</span>}
        <span className={styles.label}>{item.label}</span>
        <ChevronRight size={14} strokeWidth={2} className={styles.arrow} />
      </button>
      {open && (
        <div className={`${styles.menu} ${styles.submenu}`}>
          <Entries items={item.submenu ?? []} onClose={onClose} />
        </div>
      )}
    </div>
  );
};

export default ContextMenu;
