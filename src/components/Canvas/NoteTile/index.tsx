import React from 'react';
import { EditorView, keymap, placeholder } from '@codemirror/view';
import { EditorState, Compartment } from '@codemirror/state';
import { history, defaultKeymap, historyKeymap, indentWithTab } from '@codemirror/commands';

import type { Tile } from '~/domain/interfaces/canvas.interface';
import { noteTheme } from '~/usecase/util/note';
import { readNote, writeNote } from '~/adapter/notes/notes.client';
import { stripFrontmatter, parseFrontTitle, applyFrontTitle } from '~/usecase/util/noteMeta';
import { livePreview, markdownBase } from '~/usecase/util/markdownLivePreview';

import styles from './styles.module.scss';

interface NoteTileProps {
  tile: Tile;
  wsId: string | null;
  active: boolean;
  onChange: (id: string, content: string) => void;
  onActivate: (id: string) => void;
  onEditor: (id: string, editor: EditorView | null) => void;
}

const theme = EditorView.theme({
  '&': { height: '100%', fontSize: '13px', color: 'var(--note-text)', background: 'transparent' },
  '.cm-content': { padding: '8px 10px', lineHeight: '1.5', caretColor: 'var(--note-text)' },
  '.cm-scroller': { fontFamily: 'inherit', lineHeight: '1.5' },
  '&.cm-focused': { outline: 'none' },
  '.cm-cursor': { borderLeftColor: 'var(--note-text)' },
  '.cm-placeholder': { color: 'var(--note-text)', opacity: '0.5' },
  '.cm-scroller::-webkit-scrollbar': { width: '8px' },
  '.cm-scroller::-webkit-scrollbar-track': { background: 'transparent' },
  '.cm-scroller::-webkit-scrollbar-thumb': {
    borderRadius: '4px',
    background: 'color-mix(in srgb, var(--note-text) 30%, transparent)'
  },
  '.cm-scroller::-webkit-scrollbar-thumb:hover': { background: 'color-mix(in srgb, var(--note-text) 50%, transparent)' },
  '.cm-scroller::-webkit-scrollbar-button': { display: 'none' }
});

const NoteTile = ({ tile, wsId, active, onChange, onActivate, onEditor }: NoteTileProps) => {
  const host = React.useRef<HTMLDivElement | null>(null);
  const view = React.useRef<EditorView | null>(null);
  const preview = React.useRef(new Compartment());
  const save = React.useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastEmit = React.useRef(stripFrontmatter(tile.content || ''));
  const emit = React.useRef(onChange);
  emit.current = onChange;

  const rawRef = React.useRef(tile.content || '');
  rawRef.current = tile.content || '';
  const seed = React.useRef(tile.content || '');
  const userTitle = React.useRef(tile.userTitle || '');
  userTitle.current = tile.userTitle || '';

  React.useEffect(() => {
    if (!host.current) return;

    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: '',
        extensions: [
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
          markdownBase(),
          preview.current.of(livePreview(tile.renderOnly ? 'render' : 'edit')),
          EditorView.lineWrapping,
          placeholder('Click to edit...'),
          theme,
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return;
            if (save.current) clearTimeout(save.current);
            const body = u.state.doc.toString();
            save.current = setTimeout(() => {
              const full = applyFrontTitle(body, parseFrontTitle(rawRef.current));
              lastEmit.current = body;
              if (wsId) void writeNote(wsId, tile.id, full).catch(() => {});
              emit.current(tile.id, full);
            }, 300);
          })
        ]
      })
    });

    view.current = editor;
    onEditor(tile.id, editor);

    let alive = true;
    const load = async () => {
      if (!wsId) return;
      const fromFile = await readNote(wsId, tile.id).catch(() => null);
      if (!alive) return;
      let full = fromFile && fromFile.length ? fromFile : seed.current;
      if (!parseFrontTitle(full) && userTitle.current.trim()) full = applyFrontTitle(full, userTitle.current.trim());
      const body = stripFrontmatter(full);
      lastEmit.current = body;
      if (body) editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: body } });
      emit.current(tile.id, full);
      if (full !== fromFile) void writeNote(wsId, tile.id, full).catch(() => {});
    };
    void load();

    return () => {
      alive = false;
      if (save.current) clearTimeout(save.current);
      onEditor(tile.id, null);
      editor.destroy();
      view.current = null;
    };
  }, [tile.id, wsId, onEditor]);

  React.useEffect(() => {
    view.current?.dispatch({ effects: preview.current.reconfigure(livePreview(tile.renderOnly ? 'render' : 'edit')) });
  }, [tile.renderOnly]);

  React.useEffect(() => {
    const v = view.current;
    if (!v) return;
    const nextBody = stripFrontmatter(tile.content || '');
    if (nextBody === lastEmit.current) return;
    const cur = v.state.doc.toString();
    if (cur === nextBody) return;
    lastEmit.current = nextBody;
    v.dispatch({ changes: { from: 0, to: cur.length, insert: nextBody } });
  }, [tile.content]);

  const activate = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    onActivate(tile.id);
  };

  const stopWheel = (e: React.WheelEvent) => {
    if (active) e.stopPropagation();
  };

  const tint = noteTheme(tile.color);
  const cls = active ? `${styles.editor} ${styles.scrollable}` : styles.editor;

  return (
    <div
      className={`pano-note ${styles.note}`}
      onWheel={stopWheel}
      onPointerDown={activate}
      style={{
        ['--note-body' as string]: tint.body,
        ['--note-text' as string]: tint.text
      }}
    >
      <div ref={host} className={cls} />
    </div>
  );
};

export default NoteTile;
