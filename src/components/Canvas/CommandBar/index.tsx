import type { CommandBarRow } from '~/domain/interfaces/commandBar.interface';

import React from 'react';
import { Search } from 'lucide-react';

import { buildSections, COMMAND_PREFIX } from '~/usecase/util/commandBar';
import { useCommandBarItems, type CommandBarSources } from '~/usecase/hooks/useCommandBarItems';

import Row from './Row';
import styles from './styles.module.scss';

interface CommandBarProps {
  onClose: () => void;
  sources: CommandBarSources;
}

const CommandBar = ({ sources, onClose }: CommandBarProps) => {
  const items = useCommandBarItems(sources);
  const [query, setQuery] = React.useState('');
  const [cursor, setCursor] = React.useState(0);
  const inputRef = React.useRef<HTMLInputElement | null>(null);
  const listRef = React.useRef<HTMLDivElement | null>(null);
  const pickedRef = React.useRef(false);

  React.useEffect(() => {
    const prev = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const sel = window.getSelection();
    const caret = prev && sel?.rangeCount && prev.contains(sel.getRangeAt(0).startContainer)
      ? sel.getRangeAt(0).cloneRange()
      : null;
    inputRef.current?.focus({ preventScroll: true });
    return () => {
      if (pickedRef.current || !prev?.isConnected) return;
      prev.focus({ preventScroll: true });
      if (!caret) return;
      const restored = window.getSelection();
      restored?.removeAllRanges();
      restored?.addRange(caret);
    };
  }, []);

  const sections = React.useMemo(() => buildSections(items, query), [items, query]);
  const rows = React.useMemo(() => sections.flatMap((s) => s.rows), [sections]);
  const indexOf = React.useMemo(() => new Map(rows.map((r, i) => [r.item.id, i])), [rows]);
  const at = Math.min(cursor, Math.max(rows.length - 1, 0));

  React.useEffect(() => {
    listRef.current?.querySelector(`[data-index="${at}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [at, rows]);

  const pick = (row: CommandBarRow) => {
    pickedRef.current = true;
    onClose();
    row.item.run();
  };

  const onQuery = (e: React.ChangeEvent<HTMLInputElement>) => {
    setQuery(e.target.value);
    setCursor(0);
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (!rows.length) return;
      const delta = e.key === 'ArrowDown' ? 1 : -1;
      setCursor((at + delta + rows.length) % rows.length);
      return;
    }
    if (e.key === 'Enter' && rows[at]) {
      e.preventDefault();
      pick(rows[at]);
    }
  };

  const onBackdrop = (e: React.MouseEvent) => {
    e.preventDefault();
    onClose();
  };

  const stop = (e: React.MouseEvent) => e.stopPropagation();

  const actionsOnly = query.startsWith(COMMAND_PREFIX);
  const empty = query.trim() ? 'No results' : 'No tiles in this tab';

  return (
    <div className={styles.backdrop} onMouseDown={onBackdrop}>
      <div className={styles.palette} onMouseDown={stop}>
        <div className={styles.inputRow}>
          <Search size={15} strokeWidth={1.75} className={styles.searchIcon} />
          <input
            ref={inputRef}
            value={query}
            role="combobox"
            aria-expanded
            aria-controls="command-bar-results"
            className={styles.input}
            placeholder={actionsOnly ? 'Run a command...' : 'Search tiles, tabs, commands...'}
            onChange={onQuery}
            onKeyDown={onKeyDown}
          />
        </div>
        <div ref={listRef} id="command-bar-results" role="listbox" className={styles.results}>
          {!rows.length && <div className={styles.empty}>{empty}</div>}
          {sections.map((section) => (
            <div key={section.kind} role="group" aria-label={section.title}>
              <div className={styles.header}>{section.title}</div>
              {section.rows.map((row) => {
                const index = indexOf.get(row.item.id) ?? 0;
                return (
                  <Row key={row.item.id} row={row} index={index} selected={index === at} onHover={setCursor} onPick={pick} />
                );
              })}
            </div>
          ))}
        </div>
        <div className={styles.footer}>
          <span>
            <kbd>Up/Down</kbd>Move
          </span>
          <span>
            <kbd>Enter</kbd>Open
          </span>
          <span>
            <kbd>Esc</kbd>Close
          </span>
          <span>
            <kbd>{COMMAND_PREFIX}</kbd>Commands only
          </span>
        </div>
      </div>
    </div>
  );
};

export default CommandBar;
