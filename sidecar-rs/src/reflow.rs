use serde_json::{json, Value};

use super::{cell_words, DEFAULT_FG};

const MAX_SHIFT: usize = 256;
const UNDERLINE: u32 = 4 << 24;

#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    text: String,
    runs: Vec<[u32; 3]>,
}

impl TextLine {
    pub fn text(&self) -> &str {
        &self.text
    }

    fn to_json(&self) -> Value {
        json!({ "t": self.text, "s": self.runs })
    }
}

type Cell = (String, (u32, u32));

fn invisible(cell: &Cell) -> bool {
    let (fg, bg) = cell.1;
    cell.0 == " " && bg == 0 && fg & UNDERLINE == 0
}

fn finish_line(cells: &mut Vec<Cell>) -> TextLine {
    while cells.last().is_some_and(invisible) {
        cells.pop();
    }
    let mut line = TextLine {
        text: String::new(),
        runs: Vec::new(),
    };
    for (text, (fg, bg)) in cells.drain(..) {
        let len = text.encode_utf16().count() as u32;
        line.text.push_str(&text);
        match line.runs.last_mut() {
            Some(run) if run[1] == fg && run[2] == bg => run[0] += len,
            _ => line.runs.push([len, fg, bg]),
        }
    }
    line
}

fn push_row(screen: &vt100::Screen, row: u16, cells: &mut Vec<Cell>) {
    let cols = screen.size().1;
    for col in 0..cols {
        let Some(cell) = screen.cell(row, col) else {
            cells.push((" ".into(), (DEFAULT_FG, 0)));
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let text = if cell.contents().is_empty() { " " } else { cell.contents() };
        cells.push((text.to_string(), cell_words(cell)));
    }
}

// ConPTY repaints a line by padding it with spaces to the last column, which marks the row as
// wrapped; a real soft wrap always leaves a glyph in the last cell.
fn soft_wrapped(screen: &vt100::Screen, row: u16) -> bool {
    let last = screen.cell(row, screen.size().1 - 1);
    screen.row_wrapped(row)
        && last.is_some_and(|c| c.is_wide_continuation() || !c.contents().trim().is_empty())
}

fn cells_before(screen: &vt100::Screen, row: u16, col: u16) -> usize {
    (0..col)
        .filter(|&c| !screen.cell(row, c).is_some_and(|cell| cell.is_wide_continuation()))
        .count()
}

fn utf16_len(cells: &[Cell]) -> u32 {
    cells.iter().map(|(text, _)| text.encode_utf16().count() as u32).sum()
}

pub struct TextView {
    pub lines: Vec<TextLine>,
    /// Logical line index and UTF-16 column; the column can sit past the line's trimmed end.
    pub cursor: Option<[u32; 2]>,
}

/// Joins soft-wrapped rows of the last `max_rows` rows (scrollback included) into logical lines.
pub fn logical_lines(screen: &mut vt100::Screen, max_rows: usize) -> TextView {
    let saved = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let rows = screen.size().0;
    let total = history + rows as usize;
    let (cursor_row, cursor_col) = screen.cursor_position();
    let show_cursor = !screen.hide_cursor();
    let mut next = total.saturating_sub(max_rows);
    let mut lines = Vec::new();
    let mut cells = Vec::new();
    let mut cursor = None;
    while next < total {
        let offset = history.saturating_sub(next);
        screen.set_scrollback(offset);
        let top = history - offset;
        for row in (next - top) as u16..rows {
            let before = cells.len();
            push_row(screen, row, &mut cells);
            if show_cursor && top + row as usize == history + cursor_row as usize {
                let col = cells_before(screen, row, cursor_col).min(cells.len() - before);
                let at = utf16_len(&cells[..before + col]);
                cursor = Some([lines.len() as u32, at]);
            }
            if !soft_wrapped(screen, row) {
                lines.push(finish_line(&mut cells));
            }
        }
        next = top + rows as usize;
    }
    if !cells.is_empty() {
        lines.push(finish_line(&mut cells));
    }
    let keep = cursor.map_or(0, |[line, _]| line as usize + 1);
    while lines.len() > keep && lines.last().is_some_and(|l| l.text.is_empty()) {
        lines.pop();
    }
    screen.set_scrollback(saved);
    TextView { lines, cursor }
}

fn common_prefix(old: &[TextLine], new: &[TextLine]) -> usize {
    old.iter().zip(new).take_while(|(a, b)| a == b).count()
}

/// Patch that turns `old` into `new`: drop `drop` lines, keep the next `keep`, append `lines`.
pub fn patch(old: &[TextLine], new: &[TextLine]) -> Option<Value> {
    let mut best = (0, common_prefix(old, new));
    if best.1 < new.len() {
        for drop in 1..old.len().min(MAX_SHIFT + 1) {
            if old[drop] != new[0] {
                continue;
            }
            let keep = common_prefix(&old[drop..], new);
            if keep > best.1 {
                best = (drop, keep);
            }
        }
    }
    let (drop, keep) = best;
    if drop == 0 && keep == old.len() && keep == new.len() {
        return None;
    }
    let lines: Vec<Value> = new[keep..].iter().map(TextLine::to_json).collect();
    Some(json!({ "drop": drop, "keep": keep, "lines": lines }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines_of(parser: &mut vt100::Parser, max_rows: usize) -> Vec<TextLine> {
        logical_lines(parser.screen_mut(), max_rows).lines
    }

    fn texts(lines: &[TextLine]) -> Vec<&str> {
        lines.iter().map(|l| l.text.as_str()).collect()
    }

    fn apply(old: &[TextLine], new: &[TextLine]) -> Vec<String> {
        let p = patch(old, new).unwrap();
        let (drop, keep) = (p["drop"].as_u64().unwrap() as usize, p["keep"].as_u64().unwrap() as usize);
        let mut out: Vec<String> = old[drop..drop + keep].iter().map(|l| l.text.clone()).collect();
        out.extend(p["lines"].as_array().unwrap().iter().map(|l| l["t"].as_str().unwrap().to_string()));
        out
    }

    #[test]
    fn rejoins_wrapped_rows_across_scrollback() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"one\r\nhello world, long line\r\nnext\r\n$ ");
        let lines = lines_of(&mut parser,1000);
        assert_eq!(texts(&lines), ["one", "hello world, long line", "next", "$"]);
        assert_eq!(parser.screen().scrollback(), 0);
    }

    #[test]
    fn cursor_maps_to_logical_line_and_column() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process("one\r\n日本 longer wrapped\r\n$ ".as_bytes());
        let view = logical_lines(parser.screen_mut(), 1000);
        assert_eq!(texts(&view.lines), ["one", "日本 longer wrapped", "$"]);
        assert_eq!(view.cursor, Some([2, 2]));
        parser.process(b"\x1b[2D");
        assert_eq!(logical_lines(parser.screen_mut(), 1000).cursor, Some([2, 0]));
        parser.process(b"\x1b[A\x1b[3C");
        assert_eq!(logical_lines(parser.screen_mut(), 1000).cursor, Some([1, 11]));
        parser.process(b"\r\n\r\n");
        let view = logical_lines(parser.screen_mut(), 1000);
        assert_eq!(view.lines.len(), 4);
        assert_eq!(view.cursor, Some([3, 0]));
        parser.process(b"\x1b[?25l");
        assert_eq!(logical_lines(parser.screen_mut(), 1000).cursor, None);
    }

    #[test]
    fn space_padded_rows_are_not_joined() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"$ echo hi second");
        let lines = lines_of(&mut parser,10);
        assert_eq!(texts(&lines), ["$ echo hi", "second"]);
    }

    #[test]
    fn window_keeps_only_the_last_rows() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"a\r\nb\r\nc\r\nd\r\ne");
        let lines = lines_of(&mut parser,2);
        assert_eq!(texts(&lines), ["d", "e"]);
    }

    #[test]
    fn runs_merge_equal_styles() {
        let mut parser = vt100::Parser::new(2, 20, 0);
        parser.process(b"ab\x1b[31mcd\x1b[0m");
        let lines = lines_of(&mut parser,10);
        assert_eq!(lines[0].runs.len(), 2);
        assert_eq!(lines[0].runs[0][0], 2);
        assert_eq!(lines[0].runs[1][0], 2);
    }

    #[test]
    fn patch_follows_lines_scrolling_off_the_top() {
        let mut parser = vt100::Parser::new(3, 10, 100);
        parser.process(b"a\r\nb\r\nc\r\nd");
        let old = lines_of(&mut parser,4);
        parser.process(b"\r\ne\r\nf");
        let new = lines_of(&mut parser,4);
        let p = patch(&old, &new).unwrap();
        assert_eq!(p["drop"], 2);
        assert_eq!(apply(&old, &new), ["c", "d", "e", "f"]);
        assert!(patch(&new, &new).is_none());
    }

    #[test]
    fn patch_truncates_when_lines_shrink() {
        let mut parser = vt100::Parser::new(5, 10, 0);
        parser.process(b"a\r\nb\r\nc");
        let old = lines_of(&mut parser,10);
        parser.process(b"\x1b[2J\x1b[Ha");
        let new = lines_of(&mut parser,10);
        assert_eq!(apply(&old, &new), ["a"]);
    }
}
