import type { Extension } from '@codemirror/state';
import type { ViewUpdate } from '@codemirror/view';

import { RangeSetBuilder } from '@codemirror/state';
import { Decoration, ViewPlugin, type DecorationSet, type EditorView } from '@codemirror/view';

import { langOf } from '~/usecase/util/diff';
import { loadLang, tokenizeNow } from '~/usecase/service/highlight';

const MAX_CHARS = 400_000;
const DEBOUNCE_MS = 120;

const marks = new Map<string, Decoration>();

const markFor = (color: string, italic: boolean): Decoration => {
  const key = italic ? `${color}|i` : color;
  const cached = marks.get(key);
  if (cached) return cached;

  const mark = Decoration.mark({ attributes: { style: italic ? `color:${color};font-style:italic` : `color:${color}` } });
  marks.set(key, mark);
  return mark;
};

const build = (view: EditorView, lang: string): DecorationSet => {
  const doc = view.state.doc;
  if (doc.length > MAX_CHARS) return Decoration.none;

  const tokens = tokenizeNow(doc.toString(), lang);
  if (!tokens) return Decoration.none;

  const builder = new RangeSetBuilder<Decoration>();
  const lines = Math.min(tokens.length, doc.lines);

  for (let i = 0; i < lines; i++) {
    const line = doc.line(i + 1);
    let at = line.from;

    for (const token of tokens[i]) {
      const end = Math.min(at + token.content.length, line.to);
      if (end > at && token.color) builder.add(at, end, markFor(token.color, token.fontStyle === 1));
      at = end;
      if (at >= line.to) break;
    }
  }

  return builder.finish();
};

export const shikiHighlight = (path: string): Extension =>
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet = Decoration.none;
      lang: string | null = null;
      timer: ReturnType<typeof setTimeout> | null = null;

      constructor(readonly view: EditorView) {
        void loadLang(langOf(path)).then((lang) => {
          this.lang = lang;
          this.refresh();
        });
      }

      update(u: ViewUpdate) {
        if (!u.docChanged || !this.lang) return;
        if (this.timer) clearTimeout(this.timer);
        this.timer = setTimeout(() => this.refresh(), DEBOUNCE_MS);
      }

      refresh() {
        if (!this.lang) return;
        this.decorations = build(this.view, this.lang);
        this.view.dispatch({});
      }

      destroy() {
        if (this.timer) clearTimeout(this.timer);
      }
    },
    { decorations: (plugin) => plugin.decorations }
  );
