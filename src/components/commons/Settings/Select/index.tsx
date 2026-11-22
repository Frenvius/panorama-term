import React from 'react';
import { Check, ChevronDown } from 'lucide-react';

import styles from './styles.module.scss';

export interface SelectOption {
  value: string;
  label: string;
  secondary?: string;
}

interface SelectProps {
  options: SelectOption[];
  value: string;
  onChange: (value: string) => void;
  placeholder?: string;
  disabled?: boolean;
  size?: 'md' | 'sm';
}

const Select = ({ options, value, onChange, placeholder, disabled, size = 'md' }: SelectProps) => {
  const [open, setOpen] = React.useState(false);
  const [cursor, setCursor] = React.useState(0);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const popoverRef = React.useRef<HTMLDivElement>(null);
  const [popoverStyle, setPopoverStyle] = React.useState<React.CSSProperties>({});

  const selectedOpt = options.find((o) => o.value === value);

  const openPopover = () => {
    if (disabled) return;
    const trigger = rootRef.current?.querySelector('button');
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom;
    const popoverH = Math.min(280, options.length * 36 + 8);
    const above = spaceBelow < popoverH + 8 && rect.top > popoverH + 8;
    setPopoverStyle({
      top: above ? undefined : rect.bottom + 2,
      bottom: above ? window.innerHeight - rect.top + 2 : undefined,
      left: rect.left,
      minWidth: rect.width
    });
    setCursor(options.findIndex((o) => o.value === value));
    setOpen(true);
  };

  const closePopover = () => setOpen(false);

  const selectOption = (val: string) => {
    onChange(val);
    closePopover();
  };

  React.useEffect(() => {
    if (!open) return;
    const marker = document.createElement('span');
    marker.dataset.settingsLayer = 'true';
    document.body.appendChild(marker);
    return () => marker.remove();
  }, [open]);

  React.useEffect(() => {
    if (!open) return;
    const handleOutside = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) closePopover();
    };
    document.addEventListener('mousedown', handleOutside, true);
    return () => document.removeEventListener('mousedown', handleOutside, true);
  }, [open]);

  React.useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        closePopover();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setCursor((c) => Math.min(c + 1, options.length - 1));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setCursor((c) => Math.max(c - 1, 0));
      } else if (e.key === 'Enter') {
        e.preventDefault();
        selectOption(options[cursor].value);
      }
    };
    document.addEventListener('keydown', onKey, true);
    return () => document.removeEventListener('keydown', onKey, true);
  }, [open, cursor, options]);

  const handleTriggerKey = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') {
      e.preventDefault();
      openPopover();
    }
  };

  return (
    <div ref={rootRef} className={styles.wrapper}>
      <button
        type="button"
        disabled={disabled}
        onClick={openPopover}
        onKeyDown={handleTriggerKey}
        aria-haspopup="listbox"
        aria-expanded={open}
        className={`${styles.trigger} ${open ? styles.open : ''} ${size === 'sm' ? styles.small : ''}`}
      >
        <span className={selectedOpt ? '' : styles.placeholder}>
          {selectedOpt ? selectedOpt.label : (placeholder ?? 'Select...')}
        </span>
        <ChevronDown size={size === 'sm' ? 12 : 14} strokeWidth={1.75} className={styles.chevron} />
      </button>
      {open && (
        <div
          ref={popoverRef}
          role="listbox"
          className={styles.popover}
          style={popoverStyle}
        >
          {options.map((opt, idx) => {
            const handleClick = () => selectOption(opt.value);
            return (
              <div
                key={opt.value}
                role="option"
                aria-selected={opt.value === value}
                onClick={handleClick}
                className={`${styles.option} ${idx === cursor ? styles.focused : ''}`}
              >
                <div className={styles.optionText}>
                  <span className={styles.optionLabel}>{opt.label}</span>
                  {opt.secondary && <span className={styles.optionSecondary}>{opt.secondary}</span>}
                </div>
                {opt.value === value && <Check size={13} strokeWidth={2.5} className={styles.check} />}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};

export default Select;
