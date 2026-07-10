import React from 'react';
import StarterKit from '@tiptap/starter-kit';
import TextAlign from '@tiptap/extension-text-align';
import Placeholder from '@tiptap/extension-placeholder';
import { TaskList, TaskItem } from '@tiptap/extension-list';
import { useEditor, EditorContent, type Editor } from '@tiptap/react';

import type { Tile } from '~/domain/interfaces/canvas.interface';
import { noteTextColor } from '~/usecase/util/note';

import styles from './styles.module.scss';
import './content.scss';

interface NoteTileProps {
  tile: Tile;
  onChange: (id: string, content: string) => void;
  onActivate: (id: string) => void;
  onEditor: (id: string, editor: Editor | null) => void;
}

const EXTENSIONS = [
  StarterKit,
  TaskList,
  TaskItem.configure({ nested: true }),
  TextAlign.configure({ types: ['heading', 'paragraph'] }),
  Placeholder.configure({ placeholder: 'Click to edit...' })
];

const NoteTile = ({ tile, onChange, onActivate, onEditor }: NoteTileProps) => {
  const save = React.useRef<ReturnType<typeof setTimeout> | null>(null);

  const editor = useEditor({
    extensions: EXTENSIONS,
    content: tile.content || '',
    onUpdate: ({ editor: e }) => {
      if (save.current) clearTimeout(save.current);
      save.current = setTimeout(() => onChange(tile.id, e.getHTML()), 300);
    }
  });

  React.useEffect(() => {
    onEditor(tile.id, editor);
    return () => onEditor(tile.id, null);
  }, [tile.id, editor, onEditor]);

  React.useEffect(
    () => () => {
      if (save.current) clearTimeout(save.current);
    },
    []
  );

  const activate = () => onActivate(tile.id);
  const stopWheel = (e: React.WheelEvent) => e.stopPropagation();

  return (
    <div
      className={`pano-note ${styles.note}`}
      onWheel={stopWheel}
      onPointerDown={activate}
      style={{ ['--note-text' as string]: noteTextColor(tile.color || '#fef8c4') }}
    >
      <EditorContent editor={editor} className={styles.editor} />
    </div>
  );
};

export default NoteTile;
