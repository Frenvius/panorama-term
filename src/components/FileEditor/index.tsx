import React from 'react';
import { listen } from '@tauri-apps/api/event';
import { EditorState, Compartment } from '@codemirror/state';
import { indentUnit, bracketMatching } from '@codemirror/language';
import { search, searchKeymap, openSearchPanel } from '@codemirror/search';
import { history, defaultKeymap, historyKeymap, indentWithTab } from '@codemirror/commands';
import { EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter } from '@codemirror/view';

import type { EditorRequest } from '~/usecase/util/dirtyFiles';
import { humanSize } from '~/usecase/util/bytes';
import MarkdownView from '~/components/MarkdownView';
import { markDirty } from '~/usecase/util/dirtyFiles';
import { shikiHighlight } from '~/usecase/util/shikiEditor';
import { readFileBytes, readTextFile, writeTextFile, watchFile, unwatchFile } from '~/adapter/fs/fs.client';
import { fileName, isImageFile, languageFor, codeHighlight, isMarkdownFile } from '~/usecase/util/codeEditor';

import styles from './styles.module.scss';

interface FileEditorProps {
  path: string;
  active: boolean;
}

const theme = EditorView.theme({
  '&': { height: '100%', fontSize: '12px', color: 'var(--text-1)', background: 'var(--surface-1)' },
  '.cm-content': { padding: '0', fontFamily: 'var(--font-mono)', caretColor: 'var(--text-strong)', tabSize: 4 },
  '.cm-line': { padding: '0 12px' },
  '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '18px' },
  '&.cm-focused': { outline: 'none' },
  '.cm-cursor': { borderLeftColor: 'var(--text-strong)' },
  '.cm-gutters': {
    minWidth: '48px',
    color: 'var(--text-dim)',
    fontSize: '11px',
    background: 'var(--surface-2)',
    borderRight: '1px solid var(--border-1)',
    fontVariantNumeric: 'tabular-nums'
  },
  '.cm-lineNumbers .cm-gutterElement': { padding: '0 12px 0 16px' },
  '.cm-activeLineGutter': { color: 'var(--text-muted)', background: 'transparent' },
  '.cm-activeLine': { background: 'var(--hover-soft)' },
  '.cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection': {
    background: 'color-mix(in srgb, var(--accent) 28%, transparent)'
  },
  '.cm-matchingBracket, &.cm-focused .cm-matchingBracket': {
    background: 'var(--hover-2)',
    color: 'inherit',
    outline: '1px solid var(--border-3)'
  },
  '.cm-panels': { color: 'var(--text-1)', background: 'var(--surface-3)', borderColor: 'var(--border-1)' },
  '.cm-panel.cm-search': { padding: '6px 8px', fontFamily: 'inherit', fontSize: '11.5px' },
  '.cm-panel.cm-search input, .cm-panel.cm-search button': {
    color: 'var(--text-1)',
    border: '1px solid var(--border-2)',
    borderRadius: '4px',
    background: 'var(--surface-2)'
  },
  '.cm-panel.cm-search label': { color: 'var(--text-muted)' },
  '.cm-panel.cm-search label input': { border: 'none' },
  '.cm-searchMatch': { background: 'color-mix(in srgb, var(--accent) 24%, transparent)' },
  '.cm-searchMatch.cm-searchMatch-selected': { background: 'color-mix(in srgb, var(--accent) 45%, transparent)' },
  '.cm-scroller::-webkit-scrollbar': { width: '8px', height: '8px' },
  '.cm-scroller::-webkit-scrollbar-track': { background: 'transparent' },
  '.cm-scroller::-webkit-scrollbar-thumb': {
    borderRadius: '4px',
    background: 'color-mix(in srgb, var(--text-1) 22%, transparent)'
  },
  '.cm-scroller::-webkit-scrollbar-thumb:hover': { background: 'color-mix(in srgb, var(--text-1) 38%, transparent)' },
  '.cm-scroller::-webkit-scrollbar-button': { display: 'none' }
});

const FileEditor = ({ path, active }: FileEditorProps) => {
  const host = React.useRef<HTMLDivElement | null>(null);
  const view = React.useRef<EditorView | null>(null);
  const lang = React.useRef(new Compartment());
  const saved = React.useRef('');
  const crlf = React.useRef(false);

  const [dirty, setDirty] = React.useState(false);
  const [stale, setStale] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const [preview, setPreview] = React.useState<{ url: string; size: number } | null>(null);
  const [dims, setDims] = React.useState('');
  const [rendered, setRendered] = React.useState<string | null>(null);
  const renderedRef = React.useRef(false);

  const image = isImageFile(path);
  const markdown = isMarkdownFile(path);

  const applyDisk = React.useCallback(
    (editor: EditorView, text: string) => {
      const doc = text.replace(/\r\n/g, '\n');
      const head = Math.min(editor.state.selection.main.head, doc.length);
      crlf.current = text.includes('\r\n');
      saved.current = doc;
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: doc }, selection: { anchor: head } });
      setDirty(false);
      setStale(false);
      markDirty(path, false);
    },
    [path]
  );

  const syncFromDisk = React.useCallback(async () => {
    const editor = view.current;
    if (!editor) return;
    const text = await readTextFile(path).catch(() => null);
    if (!editor || text === null) return;
    if (text.replace(/\r\n/g, '\n') === saved.current) return;
    if (editor.state.doc.toString() === saved.current) applyDisk(editor, text);
    else setStale(true);
  }, [path, applyDisk]);

  React.useEffect(() => {
    if (!image) return;
    let alive = true;
    let url: string | null = null;
    setPreview(null);
    setDims('');

    readFileBytes(path)
      .then((buf) => {
        if (!alive) return;
        url = URL.createObjectURL(new Blob([buf]));
        setPreview({ url, size: buf.byteLength });
        setError(null);
      })
      .catch((e: unknown) => alive && setError(String(e)));

    return () => {
      alive = false;
      if (url) URL.revokeObjectURL(url);
    };
  }, [path, image]);

  React.useEffect(() => {
    if (!host.current || !path || image) return;

    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: '',
        extensions: [
          lineNumbers(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          history(),
          bracketMatching(),
          indentUnit.of('  '),
          search({ top: true }),
          keymap.of([...searchKeymap, ...defaultKeymap, ...historyKeymap, indentWithTab]),
          lang.current.of([]),
          codeHighlight(),
          shikiHighlight(path),
          theme,
          EditorView.updateListener.of((u) => {
            if (!u.docChanged) return;
            if (renderedRef.current) setRendered(u.state.doc.toString());
            const changed = u.state.doc.toString() !== saved.current;
            setDirty(changed);
            markDirty(path, changed);
          })
        ]
      })
    });

    view.current = editor;

    let alive = true;
    const load = async () => {
      try {
        const text = await readTextFile(path);
        if (!alive) return;
        applyDisk(editor, text);
        setError(null);
      } catch (e) {
        if (alive) setError(String(e));
        return;
      }
      const support = await languageFor(path);
      if (alive && support) editor.dispatch({ effects: lang.current.reconfigure(support) });
    };
    void load();

    return () => {
      alive = false;
      markDirty(path, false);
      editor.destroy();
      view.current = null;
    };
  }, [path, image, applyDisk]);

  const togglePreview = React.useCallback(() => {
    const editor = view.current;
    if (!editor) return;
    renderedRef.current = !renderedRef.current;
    setRendered(renderedRef.current ? editor.state.doc.toString() : null);
  }, []);

  React.useEffect(() => {
    renderedRef.current = false;
    setRendered(null);
  }, [path]);

  React.useEffect(() => {
    const save = async (editor: EditorView) => {
      const text = editor.state.doc.toString();
      try {
        await writeTextFile(path, crlf.current ? text.replace(/\n/g, '\r\n') : text);
        saved.current = text;
        const changed = view.current?.state.doc.toString() !== text;
        setDirty(changed);
        setStale(false);
        markDirty(path, changed);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    };

    const onSave = (e: Event) => {
      const detail = (e as CustomEvent<EditorRequest>).detail;
      const editor = view.current;
      if (!editor || !path) return;
      if (detail.path ? detail.path !== path : !active) return;
      detail.handled = true;
      detail.done = save(editor);
    };

    const onFind = (e: Event) => {
      const detail = (e as CustomEvent<EditorRequest>).detail;
      const editor = view.current;
      if (!active || !editor || renderedRef.current) return;
      detail.handled = true;
      openSearchPanel(editor);
    };

    const onPreview = (e: Event) => {
      const detail = (e as CustomEvent<EditorRequest>).detail;
      if (!active || !markdown || !view.current) return;
      detail.handled = true;
      togglePreview();
    };

    window.addEventListener('editor:save', onSave);
    window.addEventListener('editor:find', onFind);
    window.addEventListener('editor:preview', onPreview);
    return () => {
      window.removeEventListener('editor:save', onSave);
      window.removeEventListener('editor:find', onFind);
      window.removeEventListener('editor:preview', onPreview);
    };
  }, [active, path, markdown, togglePreview]);

  React.useEffect(() => {
    if (!path || image) return;
    let alive = true;
    let watchId: number | null = null;
    let debounce = 0;

    void watchFile(path)
      .then((id) => {
        if (alive) watchId = id;
        else void unwatchFile(id);
      })
      .catch(() => {});

    const off = listen<{ path: string }>('file:changed', (e) => {
      if (e.payload.path !== path) return;
      window.clearTimeout(debounce);
      debounce = window.setTimeout(() => void syncFromDisk(), 150);
    });

    return () => {
      alive = false;
      window.clearTimeout(debounce);
      if (watchId !== null) void unwatchFile(watchId);
      void off.then((un) => un());
    };
  }, [path, image, syncFromDisk]);

  const reloadFromDisk = () => {
    const editor = view.current;
    if (!editor) return;
    void readTextFile(path)
      .then((text) => applyDisk(editor, text))
      .catch((e: unknown) => setError(String(e)));
  };

  const onImageLoad = (e: React.SyntheticEvent<HTMLImageElement>) => {
    const img = e.currentTarget;
    setDims(`${img.naturalWidth} x ${img.naturalHeight}`);
  };

  return (
    <div className={styles.wrap}>
      {image ? (
        <div className={styles.preview}>
          {preview && <img src={preview.url} alt={fileName(path)} onLoad={onImageLoad} />}
        </div>
      ) : (
        <div ref={host} className={rendered === null ? styles.editor : `${styles.editor} ${styles.hidden}`} />
      )}
      {rendered !== null && <MarkdownView path={path} source={rendered} />}
      <div className={styles.status}>
        <span className={styles.name}>{fileName(path)}</span>
        {markdown && (
          <button className={styles.toggle} onClick={togglePreview}>
            {rendered === null ? 'preview' : 'source'}
          </button>
        )}
        {image && preview && (
          <span className={styles.meta}>{dims ? `${dims} - ${humanSize(preview.size)}` : humanSize(preview.size)}</span>
        )}
        {error && <span className={styles.error}>{error}</span>}
        {!error && stale && (
          <button className={styles.reload} onClick={reloadFromDisk}>
            changed on disk - reload
          </button>
        )}
        {!error && !stale && dirty && <span className={styles.dirty}>unsaved</span>}
      </div>
    </div>
  );
};

export default FileEditor;
