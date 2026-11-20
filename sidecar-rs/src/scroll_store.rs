use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

// When len exceeds COMPACT_TRIGGER, rewrite keeping the last COMPACT_KEEP lines.
#[cfg(not(test))]
const COMPACT_TRIGGER: usize = 110_000;
#[cfg(not(test))]
const COMPACT_KEEP: usize = 100_000;
#[cfg(test)]
const COMPACT_TRIGGER: usize = 10;
#[cfg(test)]
const COMPACT_KEEP: usize = 5;

pub struct ScrollStore {
    path: PathBuf,
    writer: BufWriter<File>,
    write_pos: u64,
    index: Vec<u32>,
    // Absolute index of row 0 in this store (counts rows dropped since open).
    base: u64,
    // Keyed by (line_index, cols). Cleared on compact/truncate/clear.
    pub render_cache: HashMap<(usize, u16), Vec<(String, u32, u32)>>,
}

impl ScrollStore {
    /// Open or create the scroll store at `path`. Rebuilds the index from the existing file.
    pub fn open(path: PathBuf) -> Option<Self> {
        let (index, write_pos) = if path.exists() {
            match std::fs::read(&path) {
                Ok(data) => build_index(&data),
                Err(e) => {
                    eprintln!("[scroll_store] index rebuild error: {e}");
                    (Vec::new(), 0)
                }
            }
        } else {
            (Vec::new(), 0)
        };

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| eprintln!("[scroll_store] open error: {e}"))
            .ok()?;
        Some(Self {
            path,
            writer: BufWriter::new(file),
            write_pos,
            index,
            base: 0,
            render_cache: HashMap::new(),
        })
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Append a row (must not contain `\n`). Triggers compaction if over the cap.
    pub fn push(&mut self, row: &str) {
        if self.index.len() >= COMPACT_TRIGGER {
            self.compact();
        }
        if self.write_pos > u32::MAX as u64 {
            eprintln!("[scroll_store] write_pos overflow, dropping row");
            return;
        }
        let pos = self.write_pos as u32;
        if let Err(e) = self
            .writer
            .write_all(row.as_bytes())
            .and_then(|_| self.writer.write_all(b"\n"))
        {
            eprintln!("[scroll_store] write error: {e}");
            return;
        }
        self.index.push(pos);
        self.write_pos += row.len() as u64 + 1;
    }

    pub fn flush(&mut self) {
        if let Err(e) = self.writer.flush() {
            eprintln!("[scroll_store] flush error: {e}");
        }
    }

    pub fn base(&self) -> u64 {
        self.base
    }

    /// Flush and return a snapshot descriptor: (path, base, len, byte_end).
    /// Callers use `byte_end` to slice the file without racing future writes.
    pub fn snapshot(&mut self) -> (PathBuf, u64, usize, u64) {
        self.flush();
        (self.path.clone(), self.base, self.index.len(), self.write_pos)
    }

    #[cfg(test)]
    pub fn row(&mut self, index: usize) -> Option<String> {
        self.flush();
        self.row_no_flush(index)
    }

    /// Return the row at `index` without flushing (caller must have flushed already).
    pub fn row_no_flush(&self, index: usize) -> Option<String> {
        if index >= self.index.len() {
            return None;
        }
        let offset = self.index[index] as u64;
        let next_start = if index + 1 < self.index.len() {
            self.index[index + 1] as u64
        } else {
            self.write_pos
        };
        // next_start points to the byte after the '\n'; content ends one byte earlier.
        let len = next_start.saturating_sub(offset).saturating_sub(1) as usize;
        if len == 0 {
            return Some(String::new());
        }
        let mut buf = vec![0u8; len];
        let mut file = File::open(&self.path)
            .map_err(|e| eprintln!("[scroll_store] row open: {e}"))
            .ok()?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| eprintln!("[scroll_store] row seek: {e}"))
            .ok()?;
        file.read_exact(&mut buf)
            .map_err(|e| eprintln!("[scroll_store] row read: {e}"))
            .ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Keep only the first `lines` rows, truncating the file.
    pub fn truncate(&mut self, lines: usize) {
        if lines >= self.index.len() {
            return;
        }
        let new_len = self.index[lines] as u64;
        self.flush();
        match OpenOptions::new().write(true).open(&self.path) {
            Ok(f) => {
                if let Err(e) = f.set_len(new_len) {
                    eprintln!("[scroll_store] truncate set_len: {e}");
                    return;
                }
            }
            Err(e) => {
                eprintln!("[scroll_store] truncate open: {e}");
                return;
            }
        }
        self.index.truncate(lines);
        self.write_pos = new_len;
        self.render_cache.clear();
        reopen_writer(&mut self.writer, &self.path);
    }

    /// Remove all rows.
    pub fn clear(&mut self) {
        let old_len = self.index.len() as u64;
        self.flush();
        match OpenOptions::new().write(true).open(&self.path) {
            Ok(f) => {
                if let Err(e) = f.set_len(0) {
                    eprintln!("[scroll_store] clear set_len: {e}");
                    return;
                }
            }
            Err(e) => {
                eprintln!("[scroll_store] clear open: {e}");
                return;
            }
        }
        self.base += old_len;
        self.index.clear();
        self.write_pos = 0;
        self.render_cache.clear();
        reopen_writer(&mut self.writer, &self.path);
    }

    fn compact(&mut self) {
        if self.index.len() <= COMPACT_KEEP {
            return;
        }
        // Ensure all buffered writes are on disk before reading them back.
        self.flush();
        let skip = self.index.len() - COMPACT_KEEP;
        self.base += skip as u64;
        let start_offset = self.index[skip] as u64;
        let content_len = (self.write_pos - start_offset) as usize;

        let mut buf = vec![0u8; content_len];
        let read_ok = File::open(&self.path).and_then(|mut f| {
            f.seek(SeekFrom::Start(start_offset))?;
            f.read_exact(&mut buf)
        });
        if let Err(e) = read_ok {
            eprintln!("[scroll_store] compact read: {e}");
            return;
        }

        let tmp = self.path.with_extension("tmp");
        let write_ok =
            File::create(&tmp).and_then(|mut f| f.write_all(&buf).and_then(|_| f.sync_all()));
        if let Err(e) = write_ok {
            eprintln!("[scroll_store] compact write: {e}");
            return;
        }
        if let Err(e) = std::fs::rename(&tmp, &self.path) {
            eprintln!("[scroll_store] compact rename: {e}");
            return;
        }

        reopen_writer(&mut self.writer, &self.path);
        let (new_index, new_pos) = build_index(&buf);
        self.index = new_index;
        self.write_pos = new_pos;
        self.render_cache.clear();
    }
}

fn build_index(data: &[u8]) -> (Vec<u32>, u64) {
    let mut index = Vec::new();
    let mut pos = 0u32;
    let mut chunks = data.split(|&b| b == b'\n').peekable();
    while let Some(chunk) = chunks.next() {
        if chunk.is_empty() && chunks.peek().is_none() {
            break; // trailing \n gives an empty final chunk
        }
        index.push(pos);
        pos += chunk.len() as u32 + 1;
    }
    (index, data.len() as u64)
}

fn reopen_writer(writer: &mut BufWriter<File>, path: &std::path::Path) {
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(f) => *writer = BufWriter::new(f),
        Err(e) => eprintln!("[scroll_store] reopen: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_nanos();
        std::env::temp_dir().join(format!("scroll_test_{n}.scroll"))
    }

    fn open_tmp() -> (ScrollStore, PathBuf) {
        let p = tmp_path();
        let s = ScrollStore::open(p.clone()).expect("open");
        (s, p)
    }

    #[test]
    fn push_len_row_round_trip() {
        let (mut s, _p) = open_tmp();
        s.push("hello");
        s.push("world");
        assert_eq!(s.len(), 2);
        assert_eq!(s.row(0).as_deref(), Some("hello"));
        assert_eq!(s.row(1).as_deref(), Some("world"));
        assert_eq!(s.row(2), None);
    }

    #[test]
    fn truncate_keeps_first_n() {
        let (mut s, p) = open_tmp();
        for i in 0..6 {
            s.push(&format!("line{i}"));
        }
        s.truncate(3);
        assert_eq!(s.len(), 3);
        assert_eq!(s.row(0).as_deref(), Some("line0"));
        assert_eq!(s.row(2).as_deref(), Some("line2"));
        assert_eq!(s.row(3), None);

        // Verify on-disk state via reopen
        let s2 = ScrollStore::open(p).expect("reopen");
        assert_eq!(s2.len(), 3);
    }

    #[test]
    fn clear_empties_store() {
        let (mut s, _p) = open_tmp();
        s.push("a");
        s.push("b");
        s.clear();
        assert_eq!(s.len(), 0);
        assert_eq!(s.row(0), None);
    }

    #[test]
    fn compaction_keeps_last_n() {
        // cfg(test) sets COMPACT_TRIGGER=10, COMPACT_KEEP=5
        let (mut s, _p) = open_tmp();
        for i in 0..10 {
            s.push(&format!("row{i:02}"));
        }
        // len is 10 == COMPACT_TRIGGER; next push triggers compact
        s.push("row10");
        assert_eq!(s.len(), 6); // 5 kept + 1 new
        // The last 5 of the original 10 are row05..row09, then row10
        assert_eq!(s.row(0).as_deref(), Some("row05"));
        assert_eq!(s.row(4).as_deref(), Some("row09"));
        assert_eq!(s.row(5).as_deref(), Some("row10"));
    }

    #[test]
    fn base_advances_on_compact_and_clear() {
        let (mut s, _p) = open_tmp();
        assert_eq!(s.base(), 0);
        // cfg(test): COMPACT_TRIGGER=10, COMPACT_KEEP=5
        for i in 0..10 {
            s.push(&format!("row{i:02}"));
        }
        // 10 rows pushed; next push triggers compact which drops 5
        s.push("row10");
        assert_eq!(s.base(), 5);
        // clear drops remaining rows
        let remaining = s.len() as u64;
        s.clear();
        assert_eq!(s.base(), 5 + remaining);
    }

    #[test]
    fn index_rebuilt_on_reopen() {
        let (mut s, p) = open_tmp();
        s.push("alpha");
        s.push("beta");
        s.flush();
        drop(s);

        let mut s2 = ScrollStore::open(p).expect("reopen");
        assert_eq!(s2.len(), 2);
        assert_eq!(s2.row(0).as_deref(), Some("alpha"));
        assert_eq!(s2.row(1).as_deref(), Some("beta"));
    }
}
