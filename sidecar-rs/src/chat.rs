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

fn codex_text(content: &Value) -> String {
    content
        .as_array()
        .map(|list| list.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"))
        .unwrap_or_default()
}

fn codex_cmd(item: &Value) -> String {
    if let Some(cmd) = item["parsed_cmd"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|c| c["cmd"].as_str())
    {
        return cmd.to_string();
    }
    if let Some(arr) = item["command"].as_array() {
        let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
        if parts.len() >= 3 && matches!(parts[1], "-Command" | "-c" | "/C") {
            return parts.last().copied().unwrap_or_default().to_string();
        }
        return parts.join(" ");
    }
    item["command"].as_str().unwrap_or_default().to_string()
}

struct ToolPair<'a> {
    id: &'a str,
    name: &'a str,
    summary: String,
    detail: String,
    is_error: bool,
    output: String,
}

fn tool_pair(p: ToolPair<'_>) -> Vec<Value> {
    let ToolPair { id, name, summary, detail, is_error, output } = p;
    vec![
        json!({ "id": format!("{id}:call"), "kind": "tool", "toolUseId": id, "name": name, "summary": summary, "detail": detail }),
        json!({ "id": format!("{id}:res"), "kind": "toolResult", "toolUseId": id, "isError": is_error, "output": output }),
    ]
}

fn codex_item_completed(item: &Value) -> Vec<Value> {
    let id = item["id"].as_str().unwrap_or_default();
    match item["type"].as_str() {
        Some("UserMessage") => user_text(&codex_text(&item["content"]))
            .map(|text| vec![json!({ "id": id, "kind": "user", "text": text })])
            .unwrap_or_default(),
        Some("AgentMessage") => {
            let text = codex_text(&item["content"]);
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![json!({ "id": id, "kind": "assistant", "text": clip(trimmed, TEXT_CAP) })]
            }
        }
        Some("CommandExecution") => {
            let cmd = codex_cmd(item);
            let summary = clip(&one_line(&cmd), SUMMARY_CAP);
            let detail = clip(cmd.trim(), OUTPUT_CAP);
            let output = clip(item["aggregated_output"].as_str().unwrap_or_default().trim(), OUTPUT_CAP);
            let is_error = item["status"].as_str() == Some("failed")
                || item["exit_code"].as_i64().is_some_and(|c| c != 0);
            tool_pair(ToolPair { id, name: "Bash", summary, detail, is_error, output })
        }
        Some("FileChange") => {
            let (paths, diffs): (Vec<&str>, Vec<&str>) = item["changes"]
                .as_object()
                .map(|obj| {
                    let paths = obj.keys().map(String::as_str).collect();
                    let diffs = obj.values().filter_map(|v| v["unified_diff"].as_str()).collect();
                    (paths, diffs)
                })
                .unwrap_or_default();
            let summary = clip(&one_line(&paths.join(", ")), SUMMARY_CAP);
            let detail = clip(&paths.join("\n"), OUTPUT_CAP);
            let output = clip(diffs.join("\n").trim(), OUTPUT_CAP);
            let is_error = item["status"].as_str() == Some("failed");
            tool_pair(ToolPair { id, name: "Edit", summary, detail, is_error, output })
        }
        Some("McpToolCall") => {
            let name = item["tool"].as_str().unwrap_or("mcp");
            let summary = tool_summary(&item["arguments"]);
            let detail = tool_detail(&item["arguments"]);
            let output = result_text(&item["result"]["content"]);
            let is_error = item["status"].as_str() == Some("failed") || !item["error"].is_null();
            tool_pair(ToolPair { id, name, summary, detail, is_error, output })
        }
        Some("CollabAgentToolCall") => {
            let name = item["tool"].as_str().unwrap_or("agent");
            let prompt = item["prompt"].as_str().unwrap_or_default();
            let summary = clip(&one_line(prompt), SUMMARY_CAP);
            let detail = clip(prompt.trim(), OUTPUT_CAP);
            let is_error = item["status"].as_str() == Some("failed");
            tool_pair(ToolPair { id, name, summary, detail, is_error, output: String::new() })
        }
        Some("ImageView") => {
            let path = item["path"].as_str().unwrap_or_default();
            tool_pair(ToolPair {
                id,
                name: "Read",
                summary: clip(&one_line(path), SUMMARY_CAP),
                detail: clip(path, OUTPUT_CAP),
                is_error: false,
                output: String::new(),
            })
        }
        Some("Extension") => {
            let kind = item["kind"].as_str().unwrap_or("extension");
            let name = match kind {
                "web.search" => "WebSearch",
                "image_gen.generation" => "ImageGen",
                _ => kind,
            };
            let arg = item["query"]
                .as_str()
                .or_else(|| item["revisedPrompt"].as_str())
                .or_else(|| item["savedPath"].as_str())
                .unwrap_or_default();
            let output = item["savedPath"].as_str().unwrap_or_default();
            let is_error = item["status"].as_str() == Some("failed");
            tool_pair(ToolPair {
                id,
                name,
                summary: clip(&one_line(arg), SUMMARY_CAP),
                detail: clip(arg.trim(), OUTPUT_CAP),
                is_error,
                output: clip(output, OUTPUT_CAP),
            })
        }
        _ => Vec::new(),
    }
}

fn codex_entry_items(entry: &Value) -> Vec<Value> {
    if entry["type"].as_str() != Some("event_msg") {
        return Vec::new();
    }
    let payload = &entry["payload"];
    let fallback_id = entry["ordinal"]
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| entry["timestamp"].as_str().map(str::to_string))
        .unwrap_or_default();
    match payload["type"].as_str() {
        Some("user_message") => payload["message"]
            .as_str()
            .and_then(user_text)
            .map(|text| vec![json!({ "id": format!("u:{fallback_id}"), "kind": "user", "text": text })])
            .unwrap_or_default(),
        Some("agent_message") => payload["message"]
            .as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|text| vec![json!({ "id": format!("a:{fallback_id}"), "kind": "assistant", "text": clip(text, TEXT_CAP) })])
            .unwrap_or_default(),
        Some("item_completed") => codex_item_completed(&payload["item"]),
        _ => Vec::new(),
    }
}

fn entry_items(entry: &Value) -> Vec<Value> {
    if entry.get("payload").is_some_and(|p| p.is_object()) {
        return codex_entry_items(entry);
    }
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

const TAIL_WINDOW: u64 = 4 * 1024 * 1024;

fn read_from(file: &mut std::fs::File, start: u64) -> std::io::Result<Vec<u8>> {
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

fn line_end(buf: &[u8]) -> usize {
    buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1)
}

pub fn read_chunk(path: &Path, offset: Option<u64>) -> std::io::Result<ChatChunk> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if let Some(start) = offset.filter(|o| *o <= len) {
        let buf = read_from(&mut file, start)?;
        let complete = line_end(&buf);
        return Ok(ChatChunk {
            items: parse_lines(&String::from_utf8_lossy(&buf[..complete])),
            offset: start + complete as u64,
        });
    }

    let mut window = TAIL_WINDOW;
    loop {
        let from = len.saturating_sub(window);
        let buf = read_from(&mut file, from)?;
        let skip = if from == 0 { 0 } else { buf.iter().position(|b| *b == b'\n').map_or(buf.len(), |i| i + 1) };
        let complete = line_end(&buf).max(skip);
        let mut items = parse_lines(&String::from_utf8_lossy(&buf[skip..complete]));
        if items.len() >= INITIAL_ITEMS || from == 0 {
            if items.len() > INITIAL_ITEMS {
                items.drain(..items.len() - INITIAL_ITEMS);
            }
            return Ok(ChatChunk {
                items,
                offset: from + complete as u64,
            });
        }
        window = window.saturating_mul(4);
    }
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

    #[test]
    fn initial_chunk_reads_only_the_tail() {
        let dir = std::env::temp_dir().join("panorama-chat-tail-test.jsonl");
        let filler = format!("{{\"type\":\"user\",\"uuid\":\"f\",\"isMeta\":true,\"message\":{{\"content\":\"{}\"}}}}\n", "x".repeat(1024));
        let line = |i: usize| format!("{{\"type\":\"user\",\"uuid\":\"u{i}\",\"message\":{{\"content\":\"m{i}\"}}}}\n");
        let mut raw = filler.repeat(6 * 1024);
        let tail: String = (0..INITIAL_ITEMS + 5).map(line).collect();
        raw.push_str(&tail);
        std::fs::write(&dir, &raw).unwrap();

        let chunk = read_chunk(&dir, None).unwrap();
        assert_eq!(chunk.items.len(), INITIAL_ITEMS);
        assert_eq!(chunk.items[0]["text"], "m5");
        assert_eq!(chunk.offset, raw.len() as u64);
        let _ = std::fs::remove_file(dir);
    }

    #[test]
    fn maps_codex_rollout_lines_and_ignores_duplicates() {
        let raw = [
            r#"{"type":"session_meta","payload":{"id":"s1"}}"#,
            r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"duplicate"}]}}"#,
            r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"id":"u1","type":"UserMessage","content":[{"type":"Text","text":"run tests"}]}}}"#,
            r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"id":"c1","type":"CommandExecution","command":["pwsh","-Command","cargo  test"],"aggregated_output":"ok\n","exit_code":0,"status":"completed"}}}"#,
            r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"id":"f1","type":"FileChange","changes":{"src/lib.rs":{"unified_diff":"@@ -1 +1 @@\n-old\n+new\n"}},"status":"completed"}}}"#,
            r#"{"type":"event_msg","payload":{"type":"item_completed","item":{"id":"a1","type":"AgentMessage","content":[{"type":"Text","text":"All passing."}]}}}"#,
        ]
        .join("\n");
        let items = parse_lines(&raw);
        assert_eq!(items.len(), 6);
        assert_eq!(items[0], json!({ "id": "u1", "kind": "user", "text": "run tests" }));
        assert_eq!(items[1], json!({ "id": "c1:call", "kind": "tool", "toolUseId": "c1", "name": "Bash", "summary": "cargo test", "detail": "cargo  test" }));
        assert_eq!(items[2], json!({ "id": "c1:res", "kind": "toolResult", "toolUseId": "c1", "isError": false, "output": "ok" }));
        assert_eq!(items[3], json!({ "id": "f1:call", "kind": "tool", "toolUseId": "f1", "name": "Edit", "summary": "src/lib.rs", "detail": "src/lib.rs" }));
        assert_eq!(items[4], json!({ "id": "f1:res", "kind": "toolResult", "toolUseId": "f1", "isError": false, "output": "@@ -1 +1 @@\n-old\n+new" }));
        assert_eq!(items[5], json!({ "id": "a1", "kind": "assistant", "text": "All passing." }));
    }
}
