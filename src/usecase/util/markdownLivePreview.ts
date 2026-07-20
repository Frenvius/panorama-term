import { RangeSetBuilder } from '@codemirror/state';
import { syntaxTree } from '@codemirror/language';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { Decoration, ViewPlugin, WidgetType, EditorView } from '@codemirror/view';
import type { Range, Extension } from '@codemirror/state';
import type { DecorationSet, ViewUpdate } from '@codemirror/view';

const HEADING = /^ATXHeading(\d)$/;

const INLINE_MARK = new Set(['EmphasisMark', 'CodeMark', 'StrikethroughMark']);
const INLINE_STYLE: Record<string, string> = {
  Emphasis: 'cm-md-em',
  StrongEmphasis: 'cm-md-strong',
  InlineCode: 'cm-md-code',
  Strikethrough: 'cm-md-strike'
};

const overlaps = (ranges: readonly { from: number; to: number }[], from: number, to: number): boolean =>
  ranges.some((r) => r.from <= to && r.to >= from);

const skipSpaces = (view: EditorView, pos: number): number => {
  const line = view.state.doc.lineAt(pos);
  let end = pos;
  while (end < line.to && view.state.doc.sliceString(end, end + 1) === ' ') end += 1;
  return end;
};

export const toggleTaskAt = (marker: string): string => (marker.includes(' ') ? marker.replace(' ', 'x') : marker.replace(/[xX]/, ' '));

class CheckboxWidget extends WidgetType {
  constructor(
    readonly checked: boolean,
    readonly from: number,
    readonly to: number
  ) {
    super();
  }

  eq(other: CheckboxWidget): boolean {
    return other.checked === this.checked && other.from === this.from;
  }

  toDOM(view: EditorView): HTMLElement {
    const box = document.createElement('input');
    box.type = 'checkbox';
    box.checked = this.checked;
    box.className = 'cm-md-task';
    box.addEventListener('mousedown', (e) => e.preventDefault());
    box.addEventListener('change', () => {
      const marker = view.state.doc.sliceString(this.from, this.to);
      view.dispatch({ changes: { from: this.from, to: this.to, insert: toggleTaskAt(marker) }, userEvent: 'input.toggleTask' });
    });
    return box;
  }

  ignoreEvent(): boolean {
    return false;
  }
}

const build = (view: EditorView, reveal: boolean, lineReveal: boolean): DecorationSet => {
  const ranges: Range<Decoration>[] = [];
  const sel = reveal ? view.state.selection.ranges : [];

  const revealed = (from: number, to: number): boolean => {
    if (!lineReveal) return overlaps(sel, from, to);
    const line = view.state.doc.lineAt(from);
    return overlaps(sel, line.from, line.to);
  };

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;
        const heading = HEADING.exec(name);

        if (heading) {
          ranges.push(Decoration.line({ class: `cm-md-h${heading[1]}` }).range(view.state.doc.lineAt(node.from).from));
          return;
        }

        if (name === 'HeaderMark') {
          if (revealed(node.from, node.to)) return;
          ranges.push(Decoration.replace({}).range(node.from, skipSpaces(view, node.to)));
          return;
        }

        if (name === 'TaskMarker') {
          const line = view.state.doc.lineAt(node.from);
          const checked = /[xX]/.test(view.state.doc.sliceString(node.from, node.to));
          if (checked && node.to < line.to) ranges.push(Decoration.mark({ class: 'cm-md-done' }).range(node.to, line.to));
          if (revealed(node.from, node.to)) return;
          const indentEnd = line.from + (line.text.length - line.text.trimStart().length);
          if (indentEnd < node.from) ranges.push(Decoration.replace({}).range(indentEnd, node.from));
          ranges.push(Decoration.replace({ widget: new CheckboxWidget(checked, node.from, node.to) }).range(node.from, node.to));
          return;
        }

        if (name === 'QuoteMark') {
          if (revealed(node.from, node.to)) return;
          ranges.push(Decoration.replace({}).range(node.from, skipSpaces(view, node.to)));
          return;
        }

        if (INLINE_STYLE[name]) {
          ranges.push(Decoration.mark({ class: INLINE_STYLE[name] }).range(node.from, node.to));
          return;
        }

        if (INLINE_MARK.has(name) || name === 'LinkMark') {
          if (revealed(node.from, node.to)) return;
          ranges.push(Decoration.replace({}).range(node.from, node.to));
        }
      }
    });
  }

  ranges.sort((a, b) => a.from - b.from || a.value.startSide - b.value.startSide);
  const builder = new RangeSetBuilder<Decoration>();
  for (const r of ranges) builder.add(r.from, r.to, r.value);
  return builder.finish();
};

const makePlugin = (mode: RevealMode) =>
  ViewPlugin.fromClass(
    class {
      reveal: boolean;
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.reveal = mode === 'edit';
        this.decorations = build(view, this.reveal, mode === 'edit');
      }

      update(u: ViewUpdate) {
        if (mode === 'render') {
          if (u.transactions.some((t) => t.isUserEvent('input.toggleTask'))) this.reveal = false;
          else if (u.docChanged) {
            const userEdit = u.transactions.some((t) => t.isUserEvent('input') || t.isUserEvent('delete'));
            if (!userEdit) {
              this.reveal = false;
            } else {
              let inserted = false;
              u.changes.iterChanges((_fa, _ta, _fb, _tb, text) => {
                if (text.length) inserted = true;
              });
              this.reveal = inserted;
            }
          } else if (u.selectionSet) this.reveal = false;
        }
        if (u.docChanged || u.viewportChanged || u.selectionSet) this.decorations = build(u.view, this.reveal, mode === 'edit');
      }
    },
    { decorations: (v) => v.decorations }
  );

const theme = EditorView.baseTheme({
  '.cm-md-h1': { fontSize: '18px', fontWeight: '700', lineHeight: '1.3' },
  '.cm-md-h2': { fontSize: '15px', fontWeight: '700' },
  '.cm-md-h3, .cm-md-h4, .cm-md-h5, .cm-md-h6': { fontWeight: '700' },
  '.cm-md-strong': { fontWeight: '700' },
  '.cm-md-em': { fontStyle: 'italic' },
  '.cm-md-strike': { textDecoration: 'line-through', opacity: '0.7' },
  '.cm-md-code': {
    padding: '1px 4px',
    borderRadius: '4px',
    fontSize: '12px',
    background: 'rgba(0, 0, 0, 0.12)'
  },
  '.cm-md-done': { textDecoration: 'line-through', opacity: '0.55' },
  '.cm-md-task': {
    position: 'relative',
    width: '14px',
    height: '14px',
    margin: '0 6px 0 0',
    verticalAlign: '-2px',
    WebkitAppearance: 'none',
    appearance: 'none',
    boxSizing: 'border-box',
    border: '1.5px solid var(--note-text)',
    borderRadius: '3px',
    background: 'transparent',
    outline: 'none',
    cursor: 'pointer'
  },
  '.cm-md-task:focus, .cm-md-task:focus-visible': { outline: 'none' },
  '.cm-md-task:checked': { borderColor: 'var(--note-text)', background: 'var(--note-text)' },
  '.cm-md-task:checked::after': {
    content: '""',
    position: 'absolute',
    top: '1px',
    left: '4px',
    width: '3px',
    height: '7px',
    border: 'solid var(--note-body)',
    borderWidth: '0 2px 2px 0',
    transform: 'rotate(45deg)'
  }
});

export type RevealMode = 'edit' | 'render';

export const stripHiddenMarks = (text: string): string =>
  text
    .split('\n')
    .map((line) => line.replace(/^(\s*)(?:#{1,6} |> |- \[[ xX]\] )/, '$1'))
    .join('\n');

const copyFilter = EditorView.clipboardOutputFilter.of(stripHiddenMarks);

export const markdownBase = (): Extension => markdown({ base: markdownLanguage });

export const livePreview = (mode: RevealMode = 'edit'): Extension =>
  mode === 'render' ? [makePlugin(mode), theme, copyFilter] : [makePlugin(mode), theme];
