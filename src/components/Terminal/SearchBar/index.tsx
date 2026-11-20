import type { SearchMatch } from '~/domain/interfaces/pty.interface';

import React from 'react';
import { X, ChevronUp, ChevronDown } from 'lucide-react';

import { searchPty } from '~/adapter/pty/pty.client';

import styles from './styles.module.scss';

interface SearchBarProps {
  tileId: string;
  focusKey: number;
  onResults: (matches: SearchMatch[], current: number) => void;
  onClose: () => void;
}

const SearchBar = ({ tileId, focusKey, onResults, onClose }: SearchBarProps) => {
  const [query, setQuery] = React.useState('');
  const [count, setCount] = React.useState(0);
  const [current, setCurrent] = React.useState(0);
  const [truncated, setTruncated] = React.useState(false);
  const inputRef = React.useRef<HTMLInputElement>(null);
  const reqIdRef = React.useRef(0);
  const matchesRef = React.useRef<SearchMatch[]>([]);
  const debounceRef = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  React.useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    el.focus({ preventScroll: true });
    el.select();
  }, [focusKey]);

  const runSearch = React.useCallback(
    (q: string) => {
      if (!q.trim()) {
        matchesRef.current = [];
        setCount(0);
        setTruncated(false);
        onResults([], 0);
        return;
      }
      const id = ++reqIdRef.current;
      void searchPty(tileId, q).then((result) => {
        if (id !== reqIdRef.current) return;
        matchesRef.current = result.matches;
        const n = result.matches.length;
        const cur = n > 0 ? n - 1 : 0;
        setCount(n);
        setTruncated(result.truncated);
        setCurrent(cur);
        onResults(result.matches, cur);
      });
    },
    [tileId, onResults]
  );

  React.useEffect(() => {
    if (debounceRef.current) clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => runSearch(query), 150);
    return () => {
      if (debounceRef.current) clearTimeout(debounceRef.current);
    };
  }, [query, runSearch]);

  const movePrev = React.useCallback(() => {
    const matches = matchesRef.current;
    if (!matches.length) return;
    const next = current > 0 ? current - 1 : matches.length - 1;
    setCurrent(next);
    onResults(matches, next);
  }, [current, onResults]);

  const moveNext = React.useCallback(() => {
    const matches = matchesRef.current;
    if (!matches.length) return;
    const next = current < matches.length - 1 ? current + 1 : 0;
    setCurrent(next);
    onResults(matches, next);
  }, [current, onResults]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.stopPropagation();
    if (e.key === 'Escape') {
      onClose();
      return;
    }
    if (e.key === 'Enter') {
      if (e.shiftKey) moveNext();
      else movePrev();
      return;
    }
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      inputRef.current?.select();
    }
  };

  const counterText = (() => {
    if (!query.trim()) return '';
    if (count === 0) return 'No results';
    return `${current + 1}/${count}${truncated ? '+' : ''}`;
  })();

  return (
    <div className={styles.bar}>
      <input
        ref={inputRef}
        className={styles.input}
        placeholder="Search..."
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={handleKeyDown}
        spellCheck={false}
      />
      <span className={styles.counter}>{counterText}</span>
      <button className={styles.btn} onClick={movePrev} tabIndex={-1} title="Previous (Enter)">
        <ChevronUp size={14} />
      </button>
      <button className={styles.btn} onClick={moveNext} tabIndex={-1} title="Next (Shift+Enter)">
        <ChevronDown size={14} />
      </button>
      <button className={styles.btn} onClick={onClose} tabIndex={-1} title="Close (Escape)">
        <X size={14} />
      </button>
    </div>
  );
};

export default SearchBar;
