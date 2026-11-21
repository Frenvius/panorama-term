import type { OrchAgent } from '~/domain/interfaces/orch.interface';

import React from 'react';
import { X, ChevronDown } from 'lucide-react';

import { orchSettingsActions } from '~/usecase/hooks/useOrchSettings';
import styles from './styles.module.scss';

const MAX_VISIBLE = 50;

interface OptionMeta {
  value: string;
  label: string;
  agentId: string;
  model: string;
  isCustom: boolean;
}

interface UseRowMeta {
  kind: 'use';
  value: string;
  agentId: string;
  model: string;
}

type Row = OptionMeta | UseRowMeta;

const buildOptions = (agents: OrchAgent[]): OptionMeta[] => {
  const result: OptionMeta[] = [];
  for (const a of agents) {
    if (a.hidden || !a.available) continue;
    result.push({ value: a.id, label: a.id, agentId: a.id, model: '', isCustom: false });
    for (const m of a.models) {
      result.push({ value: `${a.id}:${m}`, label: `${a.id}:${m}`, agentId: a.id, model: m, isCustom: false });
    }
    const discoveredSet = new Set(a.models);
    for (const m of (a.customModels ?? [])) {
      if (!discoveredSet.has(m)) {
        result.push({ value: `${a.id}:${m}`, label: `${a.id}:${m}`, agentId: a.id, model: m, isCustom: true });
      }
    }
  }
  return result;
};

const buildUseRows = (query: string, agents: OrchAgent[], allOptions: OptionMeta[], exclude?: string[]): UseRowMeta[] => {
  const val = query.trim();
  if (!val) return [];
  if (val.includes(':')) {
    const colonIdx = val.indexOf(':');
    const agentId = val.slice(0, colonIdx);
    const model = val.slice(colonIdx + 1).trim();
    if (!model) return [];
    const knownAgent = agents.find((a) => a.id === agentId && !a.hidden && a.available);
    if (!knownAgent) return [];
    const full = `${agentId}:${model}`;
    if (allOptions.some((o) => o.value === full)) return [];
    if (exclude?.includes(full)) return [];
    return [{ kind: 'use', value: full, agentId, model }];
  }
  const rows: UseRowMeta[] = [];
  for (const a of agents) {
    if (a.hidden || !a.available) continue;
    const full = `${a.id}:${val}`;
    if (allOptions.some((o) => o.value === full)) continue;
    if (exclude?.includes(full)) continue;
    rows.push({ kind: 'use', value: full, agentId: a.id, model: val });
  }
  return rows;
};

interface ModelComboboxProps {
  agents: OrchAgent[];
  exclude?: string[];
  placeholder?: string;
  onPick: (value: string) => void;
}

const ModelCombobox = ({ agents, exclude, placeholder, onPick }: ModelComboboxProps) => {
  const [query, setQuery] = React.useState('');
  const [open, setOpen] = React.useState(false);
  const [cursor, setCursor] = React.useState(0);
  const rootRef = React.useRef<HTMLDivElement>(null);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const listRef = React.useRef<HTMLDivElement>(null);
  const [popoverStyle, setPopoverStyle] = React.useState<React.CSSProperties>({});

  const allOptions = React.useMemo(() => buildOptions(agents), [agents]);

  const filtered = React.useMemo<OptionMeta[]>(() => {
    const q = query.toLowerCase();
    const base = allOptions.filter((o) => !exclude?.includes(o.value));
    if (!q) return base.slice(0, MAX_VISIBLE);
    return base.filter((o) => o.value.toLowerCase().includes(q)).slice(0, MAX_VISIBLE);
  }, [allOptions, exclude, query]);

  const useRows = React.useMemo<UseRowMeta[]>(
    () => buildUseRows(query, agents, allOptions, exclude),
    [query, agents, allOptions, exclude]
  );

  const rows: Row[] = [...filtered, ...useRows];

  const openPopover = () => {
    const trigger = rootRef.current?.querySelector('button');
    if (!trigger) return;
    const rect = trigger.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom;
    const popoverH = Math.min(300, rows.length * 32 + 48);
    const above = spaceBelow < popoverH + 8 && rect.top > popoverH + 8;
    setPopoverStyle({
      top: above ? undefined : rect.bottom + 2,
      bottom: above ? window.innerHeight - rect.top + 2 : undefined,
      left: rect.left,
      minWidth: rect.width
    });
    setOpen(true);
    setCursor(0);
    setTimeout(() => inputRef.current?.focus(), 0);
  };

  const closePopover = () => {
    setOpen(false);
    setQuery('');
  };

  const pickOption = (opt: OptionMeta) => {
    onPick(opt.value);
    closePopover();
  };

  const pickUseRow = (row: UseRowMeta) => {
    orchSettingsActions.addCustomModel(row.agentId, row.model);
    onPick(row.value);
    closePopover();
  };

  React.useEffect(() => {
    if (!open) return;
    const handleOutside = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) closePopover();
    };
    document.addEventListener('mousedown', handleOutside);
    return () => document.removeEventListener('mousedown', handleOutside);
  }, [open]);

  React.useEffect(() => {
    if (!open) return;
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        closePopover();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setCursor((c) => Math.min(c + 1, rows.length - 1));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setCursor((c) => Math.max(c - 1, 0));
      } else if (e.key === 'Enter') {
        e.preventDefault();
        const row = rows[cursor];
        if (!row) return;
        if ('kind' in row) {
          pickUseRow(row);
        } else {
          pickOption(row);
        }
      }
    };
    document.addEventListener('keydown', handleKey, true);
    return () => document.removeEventListener('keydown', handleKey, true);
  }, [open, cursor, rows]);

  React.useEffect(() => {
    if (!open) return;
    const item = listRef.current?.querySelector(`[data-idx="${cursor}"]`);
    item?.scrollIntoView({ block: 'nearest' });
  }, [cursor, open]);

  const handleQueryChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setCursor(0);
  };

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
        onClick={openPopover}
        onKeyDown={handleTriggerKey}
        aria-haspopup="listbox"
        aria-expanded={open}
        className={`${styles.trigger} ${open ? styles.open : ''}`}
      >
        <span className={styles.placeholder}>{placeholder ?? 'Add model...'}</span>
        <ChevronDown size={14} strokeWidth={1.75} className={styles.chevron} />
      </button>
      {open && (
        <div className={styles.popover} style={popoverStyle}>
          <div className={styles.searchRow}>
            <input
              ref={inputRef}
              type="text"
              value={query}
              className={styles.searchInput}
              placeholder="Filter models..."
              onChange={handleQueryChange}
            />
            {query && (
              <button type="button" className={styles.clearBtn} onClick={() => setQuery('')}>
                <X size={12} strokeWidth={2} />
              </button>
            )}
          </div>
          <div ref={listRef} role="listbox" className={styles.list}>
            {rows.length === 0 ? (
              <div className={styles.empty}>No matches</div>
            ) : (
              rows.map((row, idx) => {
                if ('kind' in row) {
                  const handleUseClick = () => pickUseRow(row);
                  return (
                    <div
                      key={row.value}
                      role="option"
                      data-idx={idx}
                      onClick={handleUseClick}
                      className={`${styles.option} ${styles.useRow} ${idx === cursor ? styles.focused : ''}`}
                    >
                      <span>Use {row.value}</span>
                    </div>
                  );
                }
                const opt = row;
                const handleClick = () => pickOption(opt);
                const handleRemove = (e: React.MouseEvent) => {
                  e.stopPropagation();
                  orchSettingsActions.removeCustomModel(opt.agentId, opt.model);
                };
                return (
                  <div
                    key={opt.value}
                    role="option"
                    data-idx={idx}
                    onClick={handleClick}
                    className={`${styles.option} ${idx === cursor ? styles.focused : ''}`}
                  >
                    <span className={styles.optionLabel}>{opt.label}</span>
                    {opt.isCustom && (
                      <>
                        <span className={styles.customBadge}>custom</span>
                        <button
                          type="button"
                          className={styles.removeBtn}
                          onClick={handleRemove}
                          tabIndex={-1}
                        >
                          <X size={10} strokeWidth={2} />
                        </button>
                      </>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      )}
    </div>
  );
};

export default ModelCombobox;
