#[test]
fn scrollback_offset_beyond_rows_does_not_panic() {
    let rows = 20u16;
    let cols = 80u16;
    let mut parser = vt100::Parser::new(rows, cols, 10000);
    let mut seed = String::new();
    for i in 1..=200 {
        seed.push_str(&format!("line {i}\r\n"));
    }
    parser.process(seed.as_bytes());

    parser.screen_mut().set_scrollback(100);
    let screen = parser.screen();
    assert_eq!(screen.scrollback(), 100);

    let mut first = String::new();
    for c in 0..cols {
        if let Some(cell) = screen.cell(0, c) {
            first.push_str(cell.contents());
        }
    }
    assert!(first.trim_end().len() > 0, "top row should render scrollback history");
}
