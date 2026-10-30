use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::git_glance::is_inside;

const MAX_FILE_BYTES: usize = 200 * 1024;
const MAX_ENTRIES: usize = 500;
const BINARY_PROBE: usize = 8 * 1024;

// Canonicalizing both sides also stops a symlink inside the folder from reaching outside it.
fn resolve(cwd: &str, path: &str) -> Result<PathBuf, String> {
    if !path.is_empty() && !is_inside(path) {
        return Err("invalid path".into());
    }
    let root = Path::new(cwd).canonicalize().map_err(|e| e.to_string())?;
    let full = root.join(path).canonicalize().map_err(|e| e.to_string())?;
    if !full.starts_with(&root) {
        return Err("invalid path".into());
    }
    Ok(full)
}

pub fn list_json(cwd: &str, path: &str) -> Result<Value, String> {
    let dir = resolve(cwd, path)?;
    let mut entries: Vec<(bool, String, u64)> = std::fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            let meta = entry.metadata().ok()?;
            (name != ".git").then_some((meta.is_dir(), name, meta.len()))
        })
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let total = entries.len();
    entries.truncate(MAX_ENTRIES);
    let items: Vec<Value> = entries
        .into_iter()
        .map(|(dir, name, size)| json!({ "name": name, "dir": dir, "size": (!dir).then_some(size) }))
        .collect();
    Ok(json!({ "path": path, "entries": items, "total": total }))
}

pub fn read_json(cwd: &str, path: &str) -> Result<Value, String> {
    if path.is_empty() {
        return Err("missing path".into());
    }
    let file = resolve(cwd, path)?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if file.is_dir() {
        return Err("not a file".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&file)
        .and_then(|f| f.take(MAX_FILE_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    if bytes[..bytes.len().min(BINARY_PROBE)].contains(&0) {
        return Ok(json!({ "path": path, "size": size, "binary": true }));
    }
    let truncated = bytes.len() > MAX_FILE_BYTES;
    bytes.truncate(MAX_FILE_BYTES);
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok(json!({ "path": path, "size": size, "binary": false, "text": text, "truncated": truncated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!("panorama-files-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/.git")).unwrap();
        std::fs::create_dir_all(root.join("Docs")).unwrap();
        std::fs::write(root.join("b.txt"), "hello\n").unwrap();
        std::fs::write(root.join("a.bin"), [1u8, 0, 2]).unwrap();
        std::fs::write(root.join("big.txt"), "x".repeat(MAX_FILE_BYTES + 10)).unwrap();
        root
    }

    #[test]
    fn lists_reads_and_rejects_escapes() {
        let root = fixture();
        let cwd = root.to_str().unwrap();
        let list = list_json(cwd, "").unwrap();
        let names: Vec<&str> = list["entries"].as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect();
        assert_eq!(names, ["Docs", "src", "a.bin", "b.txt", "big.txt"]);
        assert!(list_json(cwd, "src").unwrap()["entries"].as_array().unwrap().is_empty());
        assert_eq!(read_json(cwd, "b.txt").unwrap()["text"], "hello\n");
        assert_eq!(read_json(cwd, "a.bin").unwrap()["binary"], true);
        let big = read_json(cwd, "big.txt").unwrap();
        assert_eq!((big["truncated"].as_bool(), big["text"].as_str().unwrap().len()), (Some(true), MAX_FILE_BYTES));
        assert!(read_json(cwd, "../b.txt").is_err());
        assert!(list_json(cwd, "src/..").is_err());
        assert!(read_json(cwd, "Docs").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
