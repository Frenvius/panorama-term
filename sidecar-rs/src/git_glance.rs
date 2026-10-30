use std::path::{Component, Path};

use serde_json::{json, Value};

use super::hidden_command;

const MAX_FILES: usize = 300;
const MAX_DIFF_BYTES: usize = 200 * 1024;

fn git(dir: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = hidden_command("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.quotepath=off"])
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if err.is_empty() { "git failed".into() } else { err });
    }
    Ok(out.stdout)
}

fn repo_root(cwd: &str) -> Result<String, String> {
    let raw = git(cwd, &["rev-parse", "--show-toplevel"]).map_err(|_| "not a git repository".to_string())?;
    Ok(String::from_utf8_lossy(&raw).trim().to_string())
}

struct Header {
    branch: String,
    upstream: Option<String>,
    ahead: u64,
    behind: u64,
}

fn track_count(track: &str, label: &str) -> u64 {
    track
        .split(',')
        .filter_map(|part| part.trim().strip_prefix(label))
        .find_map(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

fn parse_header(line: &str) -> Header {
    let rest = line.trim_start_matches("## ");
    let rest = rest.strip_prefix("No commits yet on ").unwrap_or(rest);
    let (refs, track) = match rest.split_once(" [") {
        Some((refs, track)) => (refs, track.trim_end_matches(']')),
        None => (rest, ""),
    };
    let (branch, upstream) = match refs.split_once("...") {
        Some((b, u)) => (b, Some(u.to_string())),
        None => (refs, None),
    };
    Header {
        branch: branch.to_string(),
        upstream,
        ahead: track_count(track, "ahead "),
        behind: track_count(track, "behind "),
    }
}

fn status_letter(x: char, y: char) -> char {
    match (x, y) {
        ('?', '?') => '?',
        ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => 'U',
        (' ', y) => y,
        (x, _) => x,
    }
}

fn parse_status(raw: &[u8]) -> (Option<Header>, Vec<(char, String)>) {
    let text = String::from_utf8_lossy(raw);
    let mut entries = text.split('\0').filter(|e| !e.is_empty());
    let mut header = None;
    let mut files = Vec::new();
    while let Some(entry) = entries.next() {
        if entry.starts_with("## ") {
            header = Some(parse_header(entry));
            continue;
        }
        let mut chars = entry.chars();
        let (Some(x), Some(y)) = (chars.next(), chars.next()) else { continue };
        let Some(path) = entry.get(3..) else { continue };
        if x == 'R' || x == 'C' {
            entries.next();
        }
        files.push((status_letter(x, y), path.to_string()));
    }
    (header, files)
}

pub fn status_json(cwd: &str) -> Result<Value, String> {
    let root = repo_root(cwd)?;
    let raw = git(&root, &["status", "--porcelain=v1", "-b", "-z", "--untracked-files=all"])?;
    let (header, files) = parse_status(&raw);
    let header = header.unwrap_or(Header { branch: String::new(), upstream: None, ahead: 0, behind: 0 });
    let total = files.len();
    let list: Vec<Value> = files
        .into_iter()
        .take(MAX_FILES)
        .map(|(status, path)| json!({ "status": status.to_string(), "path": path }))
        .collect();
    Ok(json!({
        "root": root,
        "branch": header.branch,
        "upstream": header.upstream,
        "ahead": header.ahead,
        "behind": header.behind,
        "total": total,
        "files": list,
    }))
}

pub fn is_inside(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn cap(mut text: String) -> (String, bool) {
    if text.len() <= MAX_DIFF_BYTES {
        return (text, false);
    }
    let mut end = MAX_DIFF_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    (text, true)
}

fn untracked_diff(root: &str, path: &str) -> Result<String, String> {
    let bytes = std::fs::read(Path::new(root).join(path)).map_err(|e| e.to_string())?;
    if bytes.contains(&0) {
        return Ok(format!("Binary file {path}"));
    }
    let body: String = String::from_utf8_lossy(&bytes).lines().map(|l| format!("+{l}\n")).collect();
    Ok(format!("--- /dev/null\n+++ b/{path}\n{body}"))
}

pub fn diff_json(cwd: &str, path: &str) -> Result<Value, String> {
    if !is_inside(path) {
        return Err("invalid path".into());
    }
    let root = repo_root(cwd)?;
    let tracked = git(&root, &["ls-files", "--error-unmatch", "--", path]).is_ok();
    let text = if tracked {
        let args = ["diff", "--no-color", "--no-ext-diff", "HEAD", "--", path];
        let raw = git(&root, &args).or_else(|_| git(&root, &["diff", "--no-color", "--no-ext-diff", "--", path]))?;
        String::from_utf8_lossy(&raw).into_owned()
    } else {
        untracked_diff(&root, path)?
    };
    let (diff, truncated) = cap(text);
    Ok(json!({ "path": path, "diff": diff, "truncated": truncated }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_tracking_and_renames() {
        let raw = b"## main...origin/main [ahead 2, behind 1]\0 M src/a.rs\0R  new.rs\0old.rs\0?? notes.md\0UU c.rs\0";
        let (header, files) = parse_status(raw);
        let header = header.unwrap();
        assert_eq!(header.branch, "main");
        assert_eq!(header.upstream.as_deref(), Some("origin/main"));
        assert_eq!((header.ahead, header.behind), (2, 1));
        let files: Vec<(char, &str)> = files.iter().map(|(s, p)| (*s, p.as_str())).collect();
        assert_eq!(files, vec![('M', "src/a.rs"), ('R', "new.rs"), ('?', "notes.md"), ('U', "c.rs")]);
    }

    #[test]
    fn parses_unborn_and_untracked_branch_headers() {
        assert_eq!(parse_header("## No commits yet on main").branch, "main");
        let h = parse_header("## feat/x");
        assert_eq!((h.branch.as_str(), h.upstream, h.ahead), ("feat/x", None, 0));
    }

    #[test]
    fn rejects_paths_leaving_the_repo() {
        assert!(is_inside("src/a.rs"));
        assert!(!is_inside("../secret"));
        assert!(!is_inside("/etc/passwd"));
        assert!(!is_inside(""));
    }
}
