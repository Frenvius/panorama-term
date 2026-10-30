use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::{json, Value};

const INITIAL_ITEMS: usize = 300;
const TEXT_CAP: usize = 8000;
const SUMMARY_CAP: usize = 160;
const OUTPUT_CAP: usize = 1500;
const SUMMARY_KEYS: &[&str] = &["command", "file_path", "path", "pattern", "url", "query", "description", "prompt"];

pub struct ChatChunk {
    pub items: Vec<Value>,
    pub offset: u64,
}

pub fn clip(text: &str, cap: usize) -> String {
    if text.chars().count() <= cap {
        return text.to_string();
    }
    let cut: String = text.chars().take(cap - 3).collect();
    format!("{cut}...")
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn tag_value<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let start = text.find(&open)? + open.len();
    let end = text[start..].find(&format!("</{tag}>"))? + start;
    Some(text[start..end].trim())
}

fn user_text(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if let Some(name) = tag_value(trimmed, "command-name") {
        let args = tag_value(trimmed, "command-args").unwrap_or_default();
        return Some(format!("{name} {args}").trim().to_string());
    }
    if trimmed.is_empty() || trimmed.starts_with('<') {
        return None;
    }
    Some(clip(trimmed, TEXT_CAP))
}

fn primary_input(input: &Value) -> Option<&str> {
    SUMMARY_KEYS.iter().find_map(|k| input.get(*k).and_then(|v| v.as_str()))
}

pub fn tool_summary(input: &Value) -> String {
    primary_input(input)
        .map(|s| clip(&one_line(s), SUMMARY_CAP))
        .unwrap_or_default()
}

fn tool_detail(input: &Value) -> String {
    primary_input(input)
        .map(|s| clip(s.trim(), OUTPUT_CAP))
        .unwrap_or_default()
}

fn result_text(content: &Value) -> String {
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(list) => list
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    clip(text.trim(), OUTPUT_CAP)
}

fn entry_items(entry: &Value) -> Vec<Value> {
    if entry["isSidechain"].as_bool() == Some(true) || entry["isMeta"].as_bool() == Some(true) {
        return Vec::new();
    }
    let id = entry["uuid"].as_str().unwrap_or_default();
    let content = &entry["message"]["content"];
    let blocks: Vec<Value> = match content {
        Value::String(s) => vec![json!({ "type": "text", "text": s })],
        Value::Array(list) => list.clone(),
        _ => return Vec::new(),
    };
    let role = entry["type"].as_str().unwrap_or_default();
    blocks
        .iter()
        .enumerate()
        .filter_map(|(i, block)| {
            let key = format!("{id}:{i}");
            match (role, block["type"].as_str()?) {
                ("user", "text") => {
                    let text = user_text(block["text"].as_str()?)?;
                    Some(json!({ "id": key, "kind": "user", "text": text }))
                }
                ("user", "tool_result") => Some(json!({
                    "id": key,
                    "kind": "toolResult",
                    "toolUseId": block["tool_use_id"],
                    "isError": block["is_error"].as_bool().unwrap_or(false),
                    "output": result_text(&block["content"]),
                })),
                ("assistant", "text") => {
                    let text = block["text"].as_str()?.trim();
                    (!text.is_empty()).then(|| json!({ "id": key, "kind": "assistant", "text": clip(text, TEXT_CAP) }))
                }
                ("assistant", "tool_use") => Some(json!({
                    "id": key,
                    "kind": "tool",
                    "toolUseId": block["id"],
                    "name": block["name"],
                    "summary": tool_summary(&block["input"]),
                    "detail": tool_detail(&block["input"]),
                })),
                _ => None,
            }
        })
        .collect()
}

pub fn parse_lines(raw: &str) -> Vec<Value> {
    raw.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .flat_map(|entry| entry_items(&entry))
        .collect()
}

pub fn read_chunk(path: &Path, offset: Option<u64>) -> std::io::Result<ChatChunk> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = offset.filter(|o| *o <= len).unwrap_or(0);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let complete = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
    let mut items = parse_lines(&String::from_utf8_lossy(&buf[..complete]));
    if offset.is_none() && items.len() > INITIAL_ITEMS {
        items.drain(..items.len() - INITIAL_ITEMS);
    }
    Ok(ChatChunk {
        items,
        offset: start + complete as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_user_assistant_and_tools() {
        let raw = [
            r#"{"type":"user","uuid":"u1","message":{"content":"fix the bug"}}"#,
            r#"{"type":"assistant","uuid":"a1","message":{"content":[{"type":"thinking","thinking":"x"},{"type":"text","text":"On it"}]}}"#,
            r#"{"type":"assistant","uuid":"a2","message":{"content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"git  status"}}]}}"#,
            r#"{"type":"user","uuid":"u2","message":{"content":[{"type":"tool_result","tool_use_id":"t1","is_error":true,"content":[{"type":"text","text":"fatal: not a repo\n"}]}]}}"#,
        ]
        .join("\n");
        let items = parse_lines(&raw);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0]["text"], "fix the bug");
        assert_eq!(items[1]["kind"], "assistant");
        assert_eq!(items[2]["summary"], "git status");
        assert_eq!(items[2]["detail"], "git  status");
        assert_eq!(items[3]["isError"], true);
        assert_eq!(items[3]["output"], "fatal: not a repo");
    }

    #[test]
    fn skips_meta_sidechain_and_tag_noise() {
        let raw = [
            r#"{"type":"user","uuid":"u1","isMeta":true,"message":{"content":"Caveat"}}"#,
            r#"{"type":"user","uuid":"u2","isSidechain":true,"message":{"content":"sub"}}"#,
            r#"{"type":"user","uuid":"u3","message":{"content":"<local-command-stdout>x</local-command-stdout>"}}"#,
            r#"{"type":"user","uuid":"u4","message":{"content":"<command-name>/compact</command-name><command-args></command-args>"}}"#,
        ]
        .join("\n");
        let items = parse_lines(&raw);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["text"], "/compact");
    }

    #[test]
    fn chunk_stops_at_last_complete_line() {
        let dir = std::env::temp_dir().join("panorama-chat-test.jsonl");
        let line = "{\"type\":\"user\",\"uuid\":\"u\",\"message\":{\"content\":\"hi\"}}\n";
        std::fs::write(&dir, format!("{line}{{\"type\":\"us")).unwrap();
        let chunk = read_chunk(&dir, Some(0)).unwrap();
        assert_eq!(chunk.items.len(), 1);
        assert_eq!(chunk.offset, line.len() as u64);
        let _ = std::fs::remove_file(dir);
    }
}
