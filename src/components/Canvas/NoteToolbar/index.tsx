import React from 'react';
import type { EditorView } from '@codemirror/view';
import { Bold, Code, List, Italic, Heading1, ListChecks, ListOrdered, Strikethrough } from 'lucide-react';

import { noteSwatch, NOTE_SWATCHES } from '~/usecase/util/note';

import styles from './styles.module.scss';

interface NoteToolbarProps {
  editor: EditorView;
  color: string;
  onColor: (color: string) => void;
}

const wrap = (view: EditorView, m: string) => {
  const { from, to } = view.state.selection.main;
  const before = view.state.doc.sliceString(Math.max(0, from - m.length), from) === m;
  const after = view.state.doc.sliceString(to, to + m.length) === m;

  if (before && after) {
    view.dispatch({
      changes: [
        { from: from - m.length, to: from, insert: '' },
        { from: to, to: to + m.length, insert: '' }
      ],
      selection: { anchor: from - m.length, head: to - m.length }
    });
  } else if (from !== to) {
    view.dispatch({
      changes: [
        { from, insert: m },
        { from: to, insert: m }
      ],
      selection: { anchor: from + m.length, head: to + m.length }
    });
  } else {
    view.dispatch({ changes: { from, insert: m + m }, selection: { anchor: from + m.length } });
  }
  view.focus();
};

const prefix = (view: EditorView, p: string) => {
  const { from, to } = view.state.selection.main;
  const first = view.state.doc.lineAt(from).number;
  const last = view.state.doc.lineAt(to).number;
  const changes = [];

  for (let n = first; n <= last; n += 1) {
    const line = view.state.doc.line(n);
    if (line.text.startsWith(p)) changes.push({ from: line.from, to: line.from + p.length, insert: '' });
    else changes.push({ from: line.from, insert: p });
  }
  view.dispatch({ changes });
  view.focus();
};

const NoteToolbar = ({ editor, color, onColor }: NoteToolbarProps) => {
  const btn = (onClick: () => void, label: string, icon: React.ReactNode) => (
    <button
      key={label}
      className={styles.btn}
      onMouseDown={(e) => e.preventDefault()}
      onClick={onClick}
      aria-label={label}
    >
      {icon}
    </button>
  );

  return (
    <div className={styles.bar} onPointerDown={(e) => e.stopPropagation()}>
      <div className={styles.swatches}>
        {NOTE_SWATCHES.map((c) => (
          <button
            key={c}
            className={color.toLowerCase() === c.toLowerCase() ? `${styles.swatch} ${styles.active}` : styles.swatch}
            style={noteSwatch(c)}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onColor(c)}
            aria-label={`Color ${c}`}
          />
        ))}
      </div>
      <span className={styles.divider} />
      {btn(() => wrap(editor, '**'), 'Bold', <Bold size={15} strokeWidth={2} />)}
      {btn(() => wrap(editor, '*'), 'Italic', <Italic size={15} strokeWidth={2} />)}
      {btn(() => wrap(editor, '~~'), 'Strikethrough', <Strikethrough size={15} strokeWidth={2} />)}
      {btn(() => wrap(editor, '`'), 'Code', <Code size={15} strokeWidth={2} />)}
      <span className={styles.divider} />
      {btn(() => prefix(editor, '# '), 'Heading', <Heading1 size={15} strokeWidth={2} />)}
      {btn(() => prefix(editor, '- '), 'Bullet list', <List size={15} strokeWidth={2} />)}
      {btn(() => prefix(editor, '1. '), 'Numbered list', <ListOrdered size={15} strokeWidth={2} />)}
      {btn(() => prefix(editor, '- [ ] '), 'Checklist', <ListChecks size={15} strokeWidth={2} />)}
    </div>
  );
};

export default NoteToolbar;
