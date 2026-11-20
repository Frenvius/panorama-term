use unicode_width::UnicodeWidthChar;

const MAX_MATCHES: usize = 10_000;

pub struct Match {
    pub line: u64,
    pub col: u16,
    pub len: u16,
}

/// Find all non-overlapping occurrences of `query` in `row`.
/// When `fold` is true, comparison is case-insensitive.
/// `col` and `len` are measured in terminal display cells.
pub fn find_in_row(row: &str, query: &[char], fold: bool) -> Vec<(u16, u16)> {
    if query.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = row.chars().collect();
    let n = chars.len();
    let q = query.len();
    let mut results = Vec::new();
    // col_widths[i] = display width of chars[i]
    let col_widths: Vec<u16> = chars.iter().map(|&c| UnicodeWidthChar::width(c).unwrap_or(0) as u16).collect();
    // prefix_col[i] = sum of display widths for chars[0..i]
    let mut prefix_col = vec![0u16; n + 1];
    for i in 0..n {
        prefix_col[i + 1] = prefix_col[i].saturating_add(col_widths[i]);
    }

    let matches_at = |text: char, pat: char| -> bool {
        if fold {
            text.to_lowercase().next() == pat.to_lowercase().next()
        } else {
            text == pat
        }
    };

    let mut i = 0;
    while i + q <= n {
        if (0..q).all(|k| matches_at(chars[i + k], query[k])) {
            let col = prefix_col[i];
            let match_width: u16 = (i..i + q).map(|k| col_widths[k]).fold(0u16, |a, b| a.saturating_add(b));
            results.push((col, match_width));
            i += q;
        } else {
            i += 1;
        }
    }
    results
}

/// Walk rows with absolute start index, collecting matches.
/// Rows with index < `start_line` are skipped (but start_line is the absolute index of rows[0]).
/// Keeps the NEWEST MAX_MATCHES (oldest dropped), sets truncated if any were dropped.
pub fn search_rows(rows: &[(u64, &str)], query: &[char], fold: bool) -> (Vec<Match>, bool) {
    let mut all: Vec<Match> = Vec::new();
    let mut truncated = false;
    for &(line, row) in rows {
        for (col, len) in find_in_row(row, query, fold) {
            all.push(Match { line, col, len });
        }
    }
    if all.len() > MAX_MATCHES {
        truncated = true;
        all.drain(..all.len() - MAX_MATCHES);
    }
    (all, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_in_row_ascii_simple() {
        let q: Vec<char> = "lo".chars().collect();
        // "hello world": "lo" appears once at char index 3 (l=3,o=4)
        let hits = find_in_row("hello world", &q, false);
        assert_eq!(hits, vec![(3, 2)]);
    }

    #[test]
    fn find_in_row_case_fold() {
        let q: Vec<char> = "abc".chars().collect();
        let hits = find_in_row("xABCy", &q, true);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, 1);
    }

    #[test]
    fn find_in_row_no_overlap() {
        let q: Vec<char> = "aa".chars().collect();
        let hits = find_in_row("aaaa", &q, false);
        // non-overlapping: positions 0 and 2
        assert_eq!(hits, vec![(0, 2), (2, 2)]);
    }

    #[test]
    fn find_in_row_empty_query() {
        let hits = find_in_row("hello", &[], false);
        assert!(hits.is_empty());
    }

    #[test]
    fn search_rows_truncates_oldest() {
        let long_row = "x".repeat(MAX_MATCHES + 10);
        let q: Vec<char> = "x".chars().collect();
        let rows: Vec<(u64, &str)> = vec![(0, &long_row)];
        let (matches, truncated) = search_rows(&rows, &q, false);
        assert!(truncated);
        assert_eq!(matches.len(), MAX_MATCHES);
        // Newest kept: last MAX_MATCHES columns
        assert_eq!(matches[0].col, 10);
    }

    #[test]
    fn search_rows_collects_across_lines() {
        let q: Vec<char> = "hi".chars().collect();
        // "hi there" hits at 0, "say hi" hits at 4, "nothing" hits "hi" at 3 (not-hi-ng)
        let rows: Vec<(u64, &str)> = vec![(0, "hi there"), (1, "say hi"), (2, "none here")];
        let (matches, truncated) = search_rows(&rows, &q, false);
        assert!(!truncated);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[1].line, 1);
    }
}
