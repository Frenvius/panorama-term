use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::claude::{
    is_interrupt_notice, is_slash_command, keep_last_turns, push_assistant, read_tail, strip_wrappers, SessionSummary,
    Turn,
};

fn sessions_root() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(Path::new(&home).join(".pi").join("agent").join("sessions"))
}

fn holds_session(dir: &Path, suffix: &str) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(suffix)) {
            return Some(path);
        }
    }
    None
}

fn find_transcript(session_id: &str) -> Option<PathBuf> {
    let root = sessions_root()?;
    let suffix = format!("_{session_id}.jsonl");
    for entry in std::fs::read_dir(&root).ok()?.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        if let Some(found) = holds_session(&dir, &suffix) {
            return Some(found);
        }
    }
    None
}

fn blocks_text(content: &Value, kind: &str) -> String {
    let Some(parts) = content.as_array() else {
        return content.as_str().unwrap_or_default().to_string();
    };
    parts
        .iter()
        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some(kind))
        .filter_map(|p| p.get(kind).and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn user_text(content: &Value) -> Option<String> {
    let stripped = strip_wrappers(&blocks_text(content, "text"));
    if stripped.is_empty() || is_slash_command(&stripped) || is_interrupt_notice(&stripped) {
        None
    } else {
        Some(stripped)
    }
}

fn assistant_parts(content: &Value) -> (String, Vec<String>) {
    let text = blocks_text(content, "text").trim().to_string();
    let tools = content
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("toolCall"))
                .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    (text, tools)
}

#[tauri::command]
pub fn pi_session_summary(session_id: String) -> Option<SessionSummary> {
    let path = find_transcript(&session_id)?;
    let (raw, partial) = read_tail(&path)?;

    let mut summary = SessionSummary {
        session_id,
        cwd: None,
        branch: None,
        model: None,
        version: None,
        ended_at: None,
        prompt_count: 0,
        partial,
        turns: Vec::new(),
    };
    let mut turns: Vec<Turn> = Vec::new();

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };

        if let Some(ts) = row.get("timestamp").and_then(|t| t.as_str()) {
            summary.ended_at = Some(ts.to_string());
        }
        if summary.cwd.is_none() {
            summary.cwd = row.get("cwd").and_then(|c| c.as_str()).map(String::from);
        }
        match row.get("type").and_then(|t| t.as_str()) {
            Some("model_change") => {
                summary.model = row.get("modelId").and_then(|m| m.as_str()).map(String::from);
            }
            Some("message") => {
                let Some(message) = row.get("message") else {
                    continue;
                };
                let Some(content) = message.get("content") else {
                    continue;
                };
                match message.get("role").and_then(|r| r.as_str()) {
                    Some("user") => {
                        if let Some(text) = user_text(content) {
                            summary.prompt_count += 1;
                            turns.push(Turn { role: "user", text, tools: Vec::new() });
                        }
                    }
                    Some("assistant") => {
                        if let Some(model) = message.get("model").and_then(|m| m.as_str()) {
                            summary.model = Some(model.to_string());
                        }
                        let (text, tools) = assistant_parts(content);
                        push_assistant(&mut turns, text, tools);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    keep_last_turns(&mut summary, turns);
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_user_and_assistant_blocks() {
        let user = serde_json::json!([{ "type": "text", "text": "do the thing" }]);
        assert_eq!(user_text(&user).as_deref(), Some("do the thing"));
        assert!(user_text(&serde_json::json!("/resume")).is_none());
        assert!(user_text(&serde_json::json!([{ "type": "image", "data": "x" }])).is_none());

        let assistant = serde_json::json!([
            { "type": "thinking", "thinking": "hidden" },
            { "type": "text", "text": "on it" },
            { "type": "toolCall", "id": "1", "name": "bash", "arguments": {} }
        ]);
        assert_eq!(assistant_parts(&assistant), ("on it".to_string(), vec!["bash".to_string()]));
    }

    #[test]
    fn plain_string_content_survives() {
        assert_eq!(user_text(&serde_json::json!("hello there")).as_deref(), Some("hello there"));
    }
}
