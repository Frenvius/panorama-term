use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;

use crate::claude::truncate;

const MAX_ENTRIES: usize = 50;
const MAX_TITLE_CHARS: usize = 120;
const HEAD_BYTES: u64 = 64 * 1024;
const TAIL_BYTES: u64 = 128 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub title: String,
    pub updated_at: i64,
}

fn home() -> Option<PathBuf> {
    std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
        .map(PathBuf::from)
}

fn same_dir(a: &str, b: &str) -> bool {
    let norm = |p: &str| {
        let p = p.strip_prefix(r"\\?\").unwrap_or(p).replace('\\', "/");
        let p = p.trim_end_matches('/').to_string();
        if cfg!(windows) { p.to_lowercase() } else { p }
    };
    norm(a) == norm(b)
}

fn one_line(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&flat, MAX_TITLE_CHARS)
}

fn read_span(path: &Path, tail: bool, bytes: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = if tail { len.saturating_sub(bytes) } else { 0 };
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.take(bytes).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn modified_ms(path: &Path) -> i64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn recent_jsonl(dir: &Path) -> Vec<(PathBuf, i64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, i64)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "jsonl"))
        .map(|p| {
            let at = modified_ms(&p);
            (p, at)
        })
        .collect();
    files.sort_by(|a, b| b.1.cmp(&a.1));
    files.truncate(MAX_ENTRIES);
    files
}

fn json_rows(text: &str) -> impl Iterator<Item = Value> + '_ {
    text.lines().filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
}

fn str_field(row: &Value, key: &str) -> Option<String> {
    row.get(key).and_then(|v| v.as_str()).filter(|s| !s.trim().is_empty()).map(String::from)
}

fn claude_title(path: &Path) -> Option<String> {
    let mut custom = None;
    let mut ai = None;
    let mut first_prompt = None;
    let head = read_span(path, false, HEAD_BYTES).unwrap_or_default();
    let tail = read_span(path, true, TAIL_BYTES).unwrap_or_default();
    for row in json_rows(&head).chain(json_rows(&tail)) {
        match row.get("type").and_then(|t| t.as_str()) {
            Some("custom-title") => custom = str_field(&row, "customTitle").or(custom),
            Some("ai-title") => ai = str_field(&row, "aiTitle").or(ai),
            Some("user") if first_prompt.is_none() => first_prompt = crate::claude::user_text(&row),
            _ => {}
        }
    }
    custom.or(ai).or(first_prompt)
}

fn claude_history(cwd: &str) -> Vec<HistoryEntry> {
    let Some(home) = crate::claude::claude_home() else {
        return Vec::new();
    };
    let dir = home.join("projects").join(crate::claude::cwd_slug(cwd));
    recent_jsonl(&dir)
        .into_iter()
        .filter_map(|(path, updated_at)| {
            let title = claude_title(&path)?;
            let id = path.file_stem()?.to_str()?.to_string();
            Some(HistoryEntry { id, title: one_line(&title), updated_at })
        })
        .collect()
}

fn pi_dir_for(cwd: &str) -> Option<PathBuf> {
    let root = crate::pi::sessions_root()?;
    std::fs::read_dir(&root).ok()?.flatten().map(|e| e.path()).find(|dir| {
        let Some((first, _)) = recent_jsonl(dir).into_iter().next() else {
            return false;
        };
        let head = read_span(&first, false, 4096).unwrap_or_default();
        let header = json_rows(&head).next();
        header.and_then(|row| str_field(&row, "cwd")).is_some_and(|c| same_dir(&c, cwd))
    })
}

fn pi_entry(path: &Path, updated_at: i64) -> Option<HistoryEntry> {
    let head = read_span(path, false, HEAD_BYTES).unwrap_or_default();
    let tail = read_span(path, true, TAIL_BYTES).unwrap_or_default();
    let mut id = None;
    let mut name = None;
    let mut first_prompt = None;
    for row in json_rows(&head).chain(json_rows(&tail)) {
        match row.get("type").and_then(|t| t.as_str()) {
            Some("session") if id.is_none() => id = str_field(&row, "id"),
            Some("session_info") => name = str_field(&row, "name").or(name),
            Some("message") if first_prompt.is_none() => {
                let message = row.get("message");
                if message.and_then(|m| m.get("role")).and_then(|r| r.as_str()) == Some("user") {
                    first_prompt = message.and_then(|m| m.get("content")).and_then(crate::pi::user_text);
                }
            }
            _ => {}
        }
    }
    let title = name.or(first_prompt)?;
    Some(HistoryEntry { id: id?, title: one_line(&title), updated_at })
}

fn pi_history(cwd: &str) -> Vec<HistoryEntry> {
    let Some(dir) = pi_dir_for(cwd) else {
        return Vec::new();
    };
    recent_jsonl(&dir)
        .into_iter()
        .filter_map(|(path, updated_at)| pi_entry(&path, updated_at))
        .collect()
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| e.to_string())
}

fn codex_state_db() -> Option<PathBuf> {
    let dir = std::env::var("CODEX_HOME")
        .map(PathBuf::from)
        .ok()
        .or_else(|| home().map(|h| h.join(".codex")))?;
    std::fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let version: u32 = name.strip_prefix("state_")?.strip_suffix(".sqlite")?.parse().ok()?;
            Some((version, e.path()))
        })
        .max_by_key(|(version, _)| *version)
        .map(|(_, path)| path)
}

fn codex_history(cwd: &str) -> Result<Vec<HistoryEntry>, String> {
    let Some(path) = codex_state_db() else {
        return Ok(Vec::new());
    };
    let conn = open_readonly(&path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, cwd, COALESCE(NULLIF(name, ''), NULLIF(title, ''), first_user_message), \
             COALESCE(updated_at_ms, updated_at * 1000) FROM threads \
             WHERE archived = 0 AND COALESCE(thread_source, 'user') = 'user' AND source IN ('cli', 'vscode') \
             ORDER BY 4 DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, i64>(3)?))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows
        .flatten()
        .filter(|(_, dir, title, _)| same_dir(dir, cwd) && title.as_deref().is_some_and(|t| !t.trim().is_empty()))
        .take(MAX_ENTRIES)
        .map(|(id, _, title, updated_at)| HistoryEntry { id, title: one_line(&title.unwrap_or_default()), updated_at })
        .collect())
}

fn opencode_history(cwd: &str) -> Result<Vec<HistoryEntry>, String> {
    let Some(path) = home().map(|h| h.join(".local").join("share").join("opencode").join("opencode.db")) else {
        return Ok(Vec::new());
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let conn = open_readonly(&path)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, directory, title, time_updated FROM session \
             WHERE parent_id IS NULL AND time_archived IS NULL ORDER BY time_updated DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows
        .flatten()
        .filter(|(_, dir, _, _)| same_dir(dir, cwd))
        .take(MAX_ENTRIES)
        .map(|(id, _, title, updated_at)| HistoryEntry { id, title: one_line(&title), updated_at })
        .collect())
}

fn kimi_entry(state: &Path, cwd: &str) -> Option<HistoryEntry> {
    let row: Value = serde_json::from_str(&std::fs::read_to_string(state).ok()?).ok()?;
    if row.get("archived").and_then(|a| a.as_bool()) == Some(true) || !same_dir(&str_field(&row, "cwd")?, cwd) {
        return None;
    }
    let title = str_field(&row, "title").or_else(|| str_field(&row, "lastPrompt"))?;
    let updated_at = row.get("updatedAt").and_then(|u| u.as_i64()).unwrap_or_else(|| modified_ms(state));
    Some(HistoryEntry { id: str_field(&row, "id")?, title: one_line(&title), updated_at })
}

fn kimi_history(cwd: &str) -> Vec<HistoryEntry> {
    let Some(root) = home().map(|h| h.join(".kimi-code").join("sessions")) else {
        return Vec::new();
    };
    let Ok(workspaces) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut entries: Vec<HistoryEntry> = workspaces
        .flatten()
        .filter_map(|w| std::fs::read_dir(w.path()).ok())
        .flat_map(|sessions| sessions.flatten())
        .filter_map(|s| kimi_entry(&s.path().join("state.json"), cwd))
        .collect();
    entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    entries.truncate(MAX_ENTRIES);
    entries
}

#[tauri::command]
pub async fn agent_history(agent: String, cwd: String) -> Result<Vec<HistoryEntry>, String> {
    crate::blocking(move || {
        let mut entries = match agent.as_str() {
            "claude" => claude_history(&cwd),
            "pi" => pi_history(&cwd),
            "codex" => codex_history(&cwd)?,
            "opencode" => opencode_history(&cwd)?,
            "kimi" => kimi_history(&cwd),
            other => return Err(format!("no history source for {other}")),
        };
        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(entries)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn same_dir_ignores_verbatim_prefix_separators_and_case() {
        assert!(same_dir(r"\\?\D:\workspace\projects\x", "d:/workspace/projects/x/"));
        assert!(!same_dir(r"D:\workspace\projects\x", r"D:\workspace\projects\xy"));
    }

    #[test]
    fn one_line_flattens_whitespace() {
        assert_eq!(one_line("fix\n  the   bug\n"), "fix the bug");
    }
}
