use vt100::{Cell, Color, Screen};

use crate::reflow::soft_wrapped;

const HEADER: &[u8] = b"\x1b_panorama-history;";
const HEADER_END: &[u8] = b"\x1b\\";

#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    fg: Color,
    bg: Color,
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    inverse: bool,
}

impl Style {
    fn of(cell: &Cell) -> Self {
        Self {
            fg: cell.fgcolor(),
            bg: cell.bgcolor(),
            bold: cell.bold(),
            dim: cell.dim(),
            italic: cell.italic(),
            underline: cell.underline(),
            inverse: cell.inverse(),
        }
    }

    fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(b"\x1b[0");
        let flags = [(self.bold, "1"), (self.dim, "2"), (self.italic, "3"), (self.underline, "4"), (self.inverse, "7")];
        for (_, code) in flags.iter().filter(|(on, _)| *on) {
            out.push(b';');
            out.extend_from_slice(code.as_bytes());
        }
        write_color(out, self.fg, 30);
        write_color(out, self.bg, 40);
        out.push(b'm');
    }
}

fn write_color(out: &mut Vec<u8>, color: Color, base: u8) {
    let code = match color {
        Color::Default => return,
        Color::Idx(i) if i < 8 => format!(";{}", base + i),
        Color::Idx(i) if i < 16 => format!(";{}", base + 60 + i - 8),
        Color::Idx(i) => format!(";{};5;{i}", base + 8),
        Color::Rgb(r, g, b) => format!(";{};2;{r};{g};{b}", base + 8),
    };
    out.extend_from_slice(code.as_bytes());
}

fn visible(cell: &Cell) -> bool {
    cell.has_contents() || cell.bgcolor() != Color::Default || cell.inverse()
}

fn write_row(out: &mut Vec<u8>, screen: &Screen, row: u16, style: &mut Style) {
    let cols = screen.size().1;
    let end = (0..cols)
        .rev()
        .find(|&col| screen.cell(row, col).is_some_and(visible))
        .map_or(0, |col| col + 1);
    for col in 0..end {
        let Some(cell) = screen.cell(row, col) else {
            continue;
        };
        if cell.is_wide_continuation() {
            continue;
        }
        let next = Style::of(cell);
        if next != *style {
            next.write(out);
            *style = next;
        }
        out.extend_from_slice(if cell.has_contents() { cell.contents() } else { " " }.as_bytes());
    }
}

fn main_grid_bytes(screen: &mut Screen) -> Vec<u8> {
    let saved = screen.scrollback();
    screen.set_scrollback(usize::MAX);
    let history = screen.scrollback();
    let rows = screen.size().0;
    let total = history + rows as usize;
    let mut out = Vec::new();
    let mut style = Style::default();
    let mut next = 0;
    while next < total {
        let offset = history.saturating_sub(next);
        screen.set_scrollback(offset);
        let top = history - offset;
        for row in (next - top) as u16..rows {
            write_row(&mut out, screen, row, &mut style);
            if soft_wrapped(screen, row) {
                continue;
            }
            if style != Style::default() {
                out.extend_from_slice(b"\x1b[m");
                style = Style::default();
            }
            if top + (row as usize) + 1 < total {
                out.extend_from_slice(b"\r\n");
            }
        }
        next = top + rows as usize;
    }
    screen.set_scrollback(0);
    out.extend_from_slice(&screen.cursor_state_formatted());
    screen.set_scrollback(saved);
    out
}

pub fn main_history(screen: &mut Screen) -> Vec<u8> {
    if !screen.alternate_screen() {
        return main_grid_bytes(screen);
    }
    let (rows, cols) = screen.size();
    let mut main = vt100::Parser::new(rows, cols, 0);
    *main.screen_mut() = screen.clone();
    main.process(b"\x1b[?1049l");
    main_grid_bytes(main.screen_mut())
}

pub fn screen_tail(screen: &Screen) -> Vec<u8> {
    if !screen.alternate_screen() {
        return screen.input_mode_formatted();
    }
    let mut out = b"\x1b[?1049h".to_vec();
    out.extend_from_slice(&screen.state_formatted());
    out.extend_from_slice(&screen.cursor_state_formatted());
    out
}

#[cfg(test)]
fn snapshot(screen: &mut Screen) -> Vec<u8> {
    let mut out = main_history(screen);
    out.extend_from_slice(&screen_tail(screen));
    out
}

// The header rides in an APC string so a parser replaying the file skips it.
pub fn encode(pid: u32, total: u64, body: &[u8]) -> Vec<u8> {
    let mut out = HEADER.to_vec();
    out.extend_from_slice(format!("{pid};{total}").as_bytes());
    out.extend_from_slice(HEADER_END);
    out.extend_from_slice(body);
    out
}

pub fn decode(data: &[u8]) -> Option<(u32, u64, &[u8])> {
    let rest = data.strip_prefix(HEADER)?;
    let end = rest.windows(HEADER_END.len()).position(|w| w == HEADER_END)?;
    let (pid, total) = std::str::from_utf8(&rest[..end]).ok()?.split_once(';')?;
    Some((pid.parse().ok()?, total.parse().ok()?, &rest[end + HEADER_END.len()..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rebuild(bytes: &[u8], rows: u16, cols: u16) -> vt100::Parser {
        let mut parser = vt100::Parser::new(rows, cols, 1000);
        parser.process(bytes);
        parser
    }

    fn all_rows(parser: &mut vt100::Parser) -> Vec<String> {
        let cols = parser.screen().size().1;
        parser.screen_mut().set_scrollback(usize::MAX);
        let history = parser.screen().scrollback();
        let mut out = Vec::new();
        for offset in (0..=history).rev() {
            parser.screen_mut().set_scrollback(offset);
            if offset > 0 {
                out.push(parser.screen().rows(0, cols).next().unwrap());
            } else {
                out.extend(parser.screen().rows(0, cols));
            }
        }
        out
    }

    #[test]
    fn main_screen_round_trips_with_scrollback_colors_and_cursor() {
        let mut src = vt100::Parser::new(4, 12, 1000);
        src.process(b"one\r\n\x1b[31;1mred\x1b[0m two\r\nhello world, wraps\r\n\x1b[44m  \x1b[m\r\nlast\r\n$ ab\x1b[D");
        let bytes = snapshot(src.screen_mut());
        let mut dst = rebuild(&bytes, 4, 12);
        assert_eq!(all_rows(&mut dst), all_rows(&mut src));
        dst.screen_mut().set_scrollback(0);
        src.screen_mut().set_scrollback(0);
        assert_eq!(dst.screen().contents_formatted(), src.screen().contents_formatted());
        assert_eq!(dst.screen().cursor_position(), src.screen().cursor_position());
        assert!(dst.screen().row_wrapped(0) == src.screen().row_wrapped(0));
    }

    #[test]
    fn alternate_screen_keeps_main_history_and_alt_contents() {
        let mut src = vt100::Parser::new(4, 20, 1000);
        src.process(b"a\r\nb\r\nc\r\nd\r\n$ claude\x1b[?1049h\x1b[2J\x1b[2;3Hagent ui\x1b[?2004h");
        let bytes = snapshot(src.screen_mut());
        let mut dst = rebuild(&bytes, 4, 20);
        assert!(dst.screen().alternate_screen());
        assert_eq!(dst.screen().contents(), src.screen().contents());
        assert_eq!(dst.screen().cursor_position(), src.screen().cursor_position());
        assert!(dst.screen().bracketed_paste());
        dst.process(b"\x1b[?1049l");
        src.process(b"\x1b[?1049l");
        assert_eq!(all_rows(&mut dst), all_rows(&mut src));
        assert_eq!(dst.screen().cursor_position(), src.screen().cursor_position());
    }

    #[test]
    fn header_round_trips_and_is_invisible() {
        let encoded = encode(42, 9001, b"hi");
        assert_eq!(decode(&encoded), Some((42, 9001, &b"hi"[..])));
        assert_eq!(rebuild(&encoded, 2, 10).screen().contents(), "hi");
        assert_eq!(decode(b"plain bytes"), None);
    }
}
