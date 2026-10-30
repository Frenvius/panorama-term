use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use serde_json::{json, Value};

const SNIPPET_MAX: usize = 280;
const PROMPT_GLYPHS: [char; 6] = ['❯', '›', '➜', 'λ', '$', '#'];

fn clean(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(SNIPPET_MAX)
        .collect()
}

fn agent_snippets() -> MutexGuard<'static, HashMap<String, Value>> {
    static SNIPPETS: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    SNIPPETS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn record_agent_event(tile_id: &str, payload: &Value) {
    let field = |k: &str| payload[k].as_str().unwrap_or("");
    let (role, text) = match field("event") {
        "prompt-submit" => ("user", clean(field("query"))),
        "stop" => ("agent", clean(field("response"))),
        "permission" => {
            let parts: Vec<&str> = [field("tool_name"), field("message")]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
            ("agent", clean(&parts.join(": ")))
        }
        _ => return,
    };
    if text.is_empty() {
        return;
    }
    agent_snippets().insert(tile_id.to_string(), json!({ "role": role, "text": text }));
}

pub fn agent_snippet(tile_id: &str) -> Option<Value> {
    agent_snippets().get(tile_id).cloned()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '.' | '-')
}

fn user_host_command(line: &str) -> Option<&str> {
    let (user, rest) = line.split_once('@')?;
    if user.is_empty() || !user.chars().all(is_word) {
        return None;
    }
    let host_len = rest.find(|c| !is_word(c)).unwrap_or(rest.len());
    if host_len == 0 {
        return None;
    }
    let mut rest = &rest[host_len..];
    if let Some(path) = rest.strip_prefix(':') {
        rest = &path[path.find(['$', '#'])?..];
    }
    rest.strip_prefix(['$', '#'])
}

fn glyph_command(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(PROMPT_GLYPHS)?;
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then_some(rest)
}

fn prompt_command(line: &str) -> Option<&str> {
    let ps = line
        .strip_prefix("PS ")
        .and_then(|rest| rest.find('>').map(|i| &rest[i + 1..]));
    ps.or_else(|| user_host_command(line))
        .or_else(|| glyph_command(line))
        .map(str::trim)
}

/// Last command typed at a recognized prompt and the last line it printed.
pub fn shell_snippet<S: AsRef<str>>(lines: &[S]) -> Option<Value> {
    let rows: Vec<&str> = lines.iter().map(|l| l.as_ref().trim_end()).collect();
    let mut end = rows.len();
    for i in (0..rows.len()).rev() {
        let Some(cmd) = prompt_command(rows[i]) else {
            continue;
        };
        if cmd.is_empty() {
            end = i;
            continue;
        }
        let header = i.checked_sub(1).map(|h| rows[h].trim()).filter(|h| !h.is_empty());
        let mut output: Vec<&str> = rows[i + 1..end].iter().copied().filter(|l| !l.trim().is_empty()).collect();
        if header.is_some() && output.last().map(|l| l.trim()) == header {
            output.pop();
        }
        let out = output.pop().map(str::trim).unwrap_or("");
        return Some(json!({ "cmd": clean(cmd), "text": clean(out) }));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_snippet_takes_last_command_and_its_last_line() {
        let lines = ["PS D:\\x> git status", "On branch main", "nothing to commit", "PS D:\\x> "];
        let s = shell_snippet(&lines).unwrap();
        assert_eq!(s["cmd"], "git status");
        assert_eq!(s["text"], "nothing to commit");
    }

    #[test]
    fn shell_snippet_reads_unix_and_glyph_prompts() {
        let unix = ["me@box:~/src$ ls", "a.txt", "me@box:~/src$"];
        assert_eq!(shell_snippet(&unix).unwrap()["cmd"], "ls");
        let glyph = ["❯ bun test", "12 pass", "❯"];
        assert_eq!(shell_snippet(&glyph).unwrap()["text"], "12 pass");
        assert!(shell_snippet(&["just output"]).is_none());
        assert!(shell_snippet(&["$echo"]).is_none());
    }

    #[test]
    fn shell_snippet_drops_repeated_prompt_header() {
        let lines = ["~/src", "❯ make", "done", "~/src", "❯"];
        assert_eq!(shell_snippet(&lines).unwrap()["text"], "done");
    }

    #[test]
    fn agent_snippet_follows_events() {
        record_agent_event("act-test", &json!({ "event": "prompt-submit", "query": "fix  the\nbug" }));
        assert_eq!(agent_snippet("act-test").unwrap(), json!({ "role": "user", "text": "fix the bug" }));
        record_agent_event("act-test", &json!({ "event": "stop", "response": "" }));
        assert_eq!(agent_snippet("act-test").unwrap()["role"], "user");
        record_agent_event("act-test", &json!({ "event": "permission", "tool_name": "Bash", "message": "ls" }));
        assert_eq!(agent_snippet("act-test").unwrap()["text"], "Bash: ls");
    }
}
