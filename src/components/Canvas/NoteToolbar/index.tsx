import React from 'react';
import type { Editor } from '@tiptap/react';
import {
  Bold,
  Code,
  List,
  Italic,
  Heading1,
  AlignLeft,
  ListChecks,
  AlignRight,
  Underline,
  AlignCenter,
  ListOrdered,
  Strikethrough
} from 'lucide-react';

import { NOTE_PALETTE } from '~/usecase/util/note';

import styles from './styles.module.scss';

interface NoteToolbarProps {
  editor: Editor;
  color: string;
  onColor: (color: string) => void;
}

const NoteToolbar = ({ editor, color, onColor }: NoteToolbarProps) => {
  const [, force] = React.useReducer((n: number) => n + 1, 0);

  React.useEffect(() => {
    editor.on('transaction', force);
    editor.on('selectionUpdate', force);
    return () => {
      editor.off('transaction', force);
      editor.off('selectionUpdate', force);
    };
  }, [editor]);

  const run = (fn: (chain: ReturnType<Editor['chain']>) => ReturnType<Editor['chain']>) => () =>
    fn(editor.chain().focus()).run();

  const btn = (active: boolean, onClick: () => void, label: string, icon: React.ReactNode) => (
    <button
      key={label}
      className={active ? `${styles.btn} ${styles.on}` : styles.btn}
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
        {NOTE_PALETTE.map((c) => (
          <button
            key={c.body}
            className={color.toLowerCase() === c.body.toLowerCase() ? `${styles.swatch} ${styles.active}` : styles.swatch}
            style={{ background: c.body }}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => onColor(c.body)}
            aria-label={`Color ${c.body}`}
          />
        ))}
      </div>
      <span className={styles.divider} />
      {btn(editor.isActive('bold'), run((c) => c.toggleBold()), 'Bold', <Bold size={15} strokeWidth={2} />)}
      {btn(editor.isActive('italic'), run((c) => c.toggleItalic()), 'Italic', <Italic size={15} strokeWidth={2} />)}
      {btn(editor.isActive('underline'), run((c) => c.toggleUnderline()), 'Underline', <Underline size={15} strokeWidth={2} />)}
      {btn(editor.isActive('strike'), run((c) => c.toggleStrike()), 'Strikethrough', <Strikethrough size={15} strokeWidth={2} />)}
      {btn(editor.isActive('code'), run((c) => c.toggleCode()), 'Code', <Code size={15} strokeWidth={2} />)}
      <span className={styles.divider} />
      {btn(editor.isActive('heading', { level: 1 }), run((c) => c.toggleHeading({ level: 1 })), 'Heading', <Heading1 size={15} strokeWidth={2} />)}
      {btn(editor.isActive('bulletList'), run((c) => c.toggleBulletList()), 'Bullet list', <List size={15} strokeWidth={2} />)}
      {btn(editor.isActive('orderedList'), run((c) => c.toggleOrderedList()), 'Numbered list', <ListOrdered size={15} strokeWidth={2} />)}
      {btn(editor.isActive('taskList'), run((c) => c.toggleTaskList()), 'Checklist', <ListChecks size={15} strokeWidth={2} />)}
      <span className={styles.divider} />
      {btn(editor.isActive({ textAlign: 'left' }), run((c) => c.setTextAlign('left')), 'Align left', <AlignLeft size={15} strokeWidth={2} />)}
      {btn(editor.isActive({ textAlign: 'center' }), run((c) => c.setTextAlign('center')), 'Align center', <AlignCenter size={15} strokeWidth={2} />)}
      {btn(editor.isActive({ textAlign: 'right' }), run((c) => c.setTextAlign('right')), 'Align right', <AlignRight size={15} strokeWidth={2} />)}
    </div>
  );
};

export default NoteToolbar;
