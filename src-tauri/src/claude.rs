use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

const TAIL_BYTES: u64 = 512 * 1024;
const MAX_TURNS: usize = 20;
const MAX_TURN_CHARS: usize = 700;
const MAX_TOOLS_PER_TURN: usize = 12;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    role: &'static str,
    text: String,
    tools: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    session_id: String,
    cwd: Option<String>,
    branch: Option<String>,
    model: Option<String>,
    version: Option<String>,
    ended_at: Option<String>,
    prompt_count: usize,
    partial: bool,
    turns: Vec<Turn>,
}

fn read_tail(path: &Path) -> Option<(String, bool)> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    if len <= TAIL_BYTES {
        let mut buf = Vec::with_capacity(len as usize);
        file.read_to_end(&mut buf).ok()?;
        return Some((String::from_utf8_lossy(&buf).into_owned(), false));
    }
    file.seek(SeekFrom::Start(len - TAIL_BYTES)).ok()?;
    let mut buf = Vec::with_capacity(TAIL_BYTES as usize);
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let start = text.find('\n').map(|at| at + 1).unwrap_or(text.len());
    Some((text[start..].to_string(), true))
}

fn claude_home() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(Path::new(&home).join(".claude"))
}

fn cwd_slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' })
        .collect()
}

fn find_transcript(session_id: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let projects = claude_home()?.join("projects");
    if let Some(cwd) = cwd {
        let direct = projects.join(cwd_slug(cwd)).join(format!("{session_id}.jsonl"));
        if direct.exists() {
            return Some(direct);
        }
    }
    for entry in std::fs::read_dir(&projects).ok()?.flatten() {
        let candidate = entry.path().join(format!("{session_id}.jsonl"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

const HARNESS_TAGS: [&str; 12] = [
    "command-name",
    "command-message",
    "command-args",
    "local-command-stdout",
    "local-command-stderr",
    "local-command-caveat",
    "system-reminder",
    "task-notification",
    "bash-input",
    "bash-stdout",
    "bash-stderr",
    "user-prompt-submit-hook",
];

fn drop_span(text: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        out.push_str(&rest[..start]);
        let after = &rest[start + open.len()..];
        match after.find(close) {
            Some(end) => rest = &after[end + close.len()..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

fn collapse_blank_lines(text: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for line in text.lines() {
        let blank = line.trim().is_empty();
        if blank && out.last().map(|l: &&str| l.trim().is_empty()).unwrap_or(true) {
            continue;
        }
        out.push(line);
    }
    while out.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    out.join("\n")
}

fn strip_wrappers(text: &str) -> String {
    let mut out = text.to_string();
    for tag in HARNESS_TAGS {
        out = drop_span(&out, &format!("<{tag}>"), &format!("</{tag}>"));
    }
    out = drop_span(&out, "[Image #", "]");
    out = drop_span(&out, "[Image: source:", "]");
    collapse_blank_lines(out.trim())
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str("...");
    out
}

fn flag(row: &Value, key: &str) -> bool {
    row.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

fn is_interrupt_notice(text: &str) -> bool {
    let t = text.trim();
    t.starts_with("[Request interrupted") && t.ends_with(']') && !t.contains('\n')
}

fn is_slash_command(text: &str) -> bool {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') || trimmed.contains('\n') {
        return false;
    }
    trimmed.split_whitespace().count() <= 2
}

fn user_text(row: &Value) -> Option<String> {
    if flag(row, "isMeta")
        || flag(row, "isSidechain")
        || flag(row, "isCompactSummary")
        || flag(row, "isVisibleInTranscriptOnly")
    {
        return None;
    }
    let content = row.get("message")?.get("content")?;
    let raw = if let Some(s) = content.as_str() {
        s.to_string()
    } else {
        let parts = content.as_array()?;
        if parts
            .iter()
            .any(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
        {
            return None;
        }
        parts
            .iter()
            .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let stripped = strip_wrappers(&raw);
    if stripped.is_empty() || is_slash_command(&stripped) || is_interrupt_notice(&stripped) {
        None
    } else {
        Some(stripped)
    }
}

fn assistant_parts(row: &Value) -> (String, Vec<String>) {
    let Some(parts) = row.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) else {
        return (String::new(), Vec::new());
    };
    let text = parts
        .iter()
        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("text"))
        .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    let tools = parts
        .iter()
        .filter(|p| p.get("type").and_then(|t| t.as_str()) == Some("tool_use"))
        .filter_map(|p| p.get("name").and_then(|n| n.as_str()))
        .map(String::from)
        .collect();
    (text.trim().to_string(), tools)
}

fn push_assistant(turns: &mut Vec<Turn>, text: String, tools: Vec<String>) {
    if text.is_empty() && tools.is_empty() {
        return;
    }
    if let Some(last) = turns.last_mut() {
        if last.role == "assistant" {
            if !text.is_empty() {
                if !last.text.is_empty() {
                    last.text.push('\n');
                }
                last.text.push_str(&text);
            }
            last.tools.extend(tools);
            return;
        }
    }
    turns.push(Turn { role: "assistant", text, tools });
}

#[tauri::command]
pub fn claude_session_summary(session_id: String, cwd: Option<String>) -> Option<SessionSummary> {
    let path = find_transcript(&session_id, cwd.as_deref())?;
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
        if let Some(branch) = row.get("gitBranch").and_then(|b| b.as_str()) {
            if !branch.is_empty() {
                summary.branch = Some(branch.to_string());
            }
        }
        if let Some(version) = row.get("version").and_then(|v| v.as_str()) {
            summary.version = Some(version.to_string());
        }

        match row.get("type").and_then(|t| t.as_str()) {
            Some("user") => {
                if let Some(text) = user_text(&row) {
                    summary.prompt_count += 1;
                    turns.push(Turn { role: "user", text, tools: Vec::new() });
                }
            }
            Some("assistant") => {
                if let Some(model) = row.get("message").and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
                    summary.model = Some(model.to_string());
                }
                let (text, tools) = assistant_parts(&row);
                push_assistant(&mut turns, text, tools);
            }
            _ => {}
        }
    }

    let start = turns.len().saturating_sub(MAX_TURNS);
    if start > 0 {
        summary.partial = true;
    }
    let mut kept = turns.split_off(start);
    for turn in kept.iter_mut() {
        turn.text = truncate(&turn.text, MAX_TURN_CHARS);
        turn.tools.truncate(MAX_TOOLS_PER_TURN);
    }
    summary.turns = kept;
    Some(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_command_wrappers_and_images() {
        let raw = "<command-name>/clear</command-name>\n<command-message>clear</command-message>real ask [Image #1] tail";
        assert_eq!(strip_wrappers(raw), "real ask  tail");
    }

    #[test]
    fn keeps_plain_prompt() {
        assert_eq!(strip_wrappers("just a prompt"), "just a prompt");
    }

    #[test]
    fn unclosed_span_drops_tail() {
        assert_eq!(strip_wrappers("keep <system-reminder>junk forever"), "keep");
    }

    #[test]
    fn task_notification_leaves_nothing() {
        let raw = "<task-notification>\n<task-id>abc</task-id>\n<status>killed</status>\n<summary>Background command \"Start dev server\" was stopped</summary>\n</task-notification>";
        assert_eq!(strip_wrappers(raw), "");
    }

    #[test]
    fn keeps_generics_that_look_like_tags() {
        let raw = "why does Vec<u8> break here? and ContentBlockParam<string>?";
        assert_eq!(strip_wrappers(raw), raw);
    }

    #[test]
    fn collapses_blank_runs() {
        assert_eq!(strip_wrappers("a\n\n\n\nb"), "a\n\nb");
    }

    #[test]
    fn skips_tool_results_and_meta() {
        let meta = serde_json::json!({ "isMeta": true, "message": { "content": "hi" } });
        assert!(user_text(&meta).is_none());
        let tool = serde_json::json!({
            "message": { "content": [{ "type": "tool_result", "content": "x" }] }
        });
        assert!(user_text(&tool).is_none());
        let real = serde_json::json!({
            "message": { "content": [{ "type": "text", "text": "do the thing" }] }
        });
        assert_eq!(user_text(&real).as_deref(), Some("do the thing"));
    }

    #[test]
    fn skips_injected_rows() {
        for key in ["isCompactSummary", "isVisibleInTranscriptOnly"] {
            let row = serde_json::json!({ key: true, "message": { "content": "This session is being continued" } });
            assert!(user_text(&row).is_none(), "{key} should be skipped");
        }
    }

    #[test]
    fn skips_bare_slash_commands() {
        assert!(is_slash_command("/compact"));
        assert!(is_slash_command("/model fable"));
        assert!(!is_slash_command("/docs is where the specs live, check it"));
        assert!(!is_slash_command("fix the bug"));
    }

    #[test]
    fn skips_interrupt_notices() {
        assert!(is_interrupt_notice("[Request interrupted by user]"));
        assert!(is_interrupt_notice("[Request interrupted by user for tool use]"));
        assert!(!is_interrupt_notice("[Request interrupted by user] but keep this real ask"));
        assert!(!is_interrupt_notice("fix the parser"));
    }

    #[test]
    fn merges_consecutive_assistant_rows_into_one_turn() {
        let mut turns = Vec::new();
        turns.push(Turn { role: "user", text: "do it".into(), tools: vec![] });
        push_assistant(&mut turns, "looking".into(), vec!["Read".into()]);
        push_assistant(&mut turns, String::new(), vec!["Edit".into()]);
        push_assistant(&mut turns, "done".into(), vec![]);
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[1].text, "looking\ndone");
        assert_eq!(turns[1].tools, vec!["Read", "Edit"]);
    }

    #[test]
    fn assistant_turn_with_only_tools_survives() {
        let mut turns = Vec::new();
        push_assistant(&mut turns, String::new(), vec!["Bash".into()]);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].tools, vec!["Bash"]);
    }

    #[test]
    fn empty_assistant_row_is_dropped() {
        let mut turns = Vec::new();
        push_assistant(&mut turns, String::new(), vec![]);
        assert!(turns.is_empty());
    }

    #[test]
    fn assistant_parts_splits_text_and_tools() {
        let row = serde_json::json!({
            "message": { "content": [
                { "type": "text", "text": "on it" },
                { "type": "tool_use", "name": "Bash", "input": {} },
                { "type": "thinking", "thinking": "hidden" }
            ]}
        });
        let (text, tools) = assistant_parts(&row);
        assert_eq!(text, "on it");
        assert_eq!(tools, vec!["Bash"]);
    }

    #[test]
    fn tail_read_keeps_last_lines_and_flags_partial() {
        let dir = std::env::temp_dir().join("panorama-tail-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.jsonl");
        let filler = "x".repeat(1000);
        let mut body = String::new();
        for i in 0..800 {
            body.push_str(&format!("{{\"n\":{i},\"pad\":\"{filler}\"}}\n"));
        }
        std::fs::write(&path, &body).unwrap();
        let (tail, partial) = read_tail(&path).unwrap();
        assert!(partial, "large file must report partial");
        assert!(tail.len() as u64 <= TAIL_BYTES);
        assert!(tail.lines().all(|l| serde_json::from_str::<Value>(l).is_ok()), "no half-parsed line");
        assert!(tail.contains("\"n\":799"), "tail must hold the newest rows");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn small_file_is_not_partial() {
        let dir = std::env::temp_dir().join("panorama-tail-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("small.jsonl");
        std::fs::write(&path, "{\"n\":1}\n{\"n\":2}\n").unwrap();
        let (tail, partial) = read_tail(&path).unwrap();
        assert!(!partial);
        assert!(tail.contains("\"n\":1"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("ação", 2), "aç...");
        assert_eq!(truncate("ação", 9), "ação");
    }
}
