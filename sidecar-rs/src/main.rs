use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

const PORT: u16 = 9777;
const DEFAULT_FG: u32 = 0xC7_D0_E0;
const DEFAULT_BG: u32 = 0x0B_0E_14;

const BASE16: [u32; 16] = [
    0x000000, 0xCD3131, 0x0DBC79, 0xE5E510, 0x2472C8, 0xBC3FBC, 0x11A8CD, 0xE5E5E5, 0x666666,
    0xF14C4C, 0x23D18B, 0xF5F543, 0x3B8EEA, 0xD670D6, 0x29B8DB, 0xE5E5E5,
];

fn idx_rgb(i: u8) -> u32 {
    match i {
        0..=15 => BASE16[i as usize],
        16..=231 => {
            let n = i - 16;
            let steps = [0u32, 95, 135, 175, 215, 255];
            let r = steps[(n / 36) as usize];
            let g = steps[((n / 6) % 6) as usize];
            let b = steps[(n % 6) as usize];
            (r << 16) | (g << 8) | b
        }
        _ => {
            let v = 8 + 10 * (i as u32 - 232);
            (v << 16) | (v << 8) | v
        }
    }
}

fn color_rgb(c: vt100::Color) -> Option<u32> {
    match c {
        vt100::Color::Default => None,
        vt100::Color::Idx(i) => Some(idx_rgb(i)),
        vt100::Color::Rgb(r, g, b) => Some(((r as u32) << 16) | ((g as u32) << 8) | b as u32),
    }
}

const DIM_MIX: f32 = 0.55;

fn blend(a: u32, b: u32, t: f32) -> u32 {
    let mix = |sa: u32, sb: u32| ((sa as f32 * (1.0 - t) + sb as f32 * t).round() as u32) & 0xFF;
    let r = mix((a >> 16) & 0xFF, (b >> 16) & 0xFF);
    let g = mix((a >> 8) & 0xFF, (b >> 8) & 0xFF);
    let bl = mix(a & 0xFF, b & 0xFF);
    (r << 16) | (g << 8) | bl
}

#[derive(Default)]
struct CwdSink {
    cwd: Option<String>,
    changed: bool,
    prompt_seen: bool,
    clipboard: Option<String>,
    title: Option<String>,
    title_changed: bool,
    responses: Vec<Vec<u8>>,
    agent_events: Vec<String>,
    notifies: Vec<(String, String)>,
    progress: Option<(u8, u8)>,
    progress_changed: bool,
    focused: Arc<AtomicBool>,
    focus_reporting: Arc<AtomicBool>,
}

impl CwdSink {
    fn take_changed(&mut self) -> Option<String> {
        if self.changed {
            self.changed = false;
            self.cwd.clone()
        } else {
            None
        }
    }

    fn take_prompt(&mut self) -> Option<String> {
        if self.prompt_seen {
            self.prompt_seen = false;
            self.cwd.clone()
        } else {
            None
        }
    }

    fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }

    fn take_title(&mut self) -> Option<String> {
        if self.title_changed {
            self.title_changed = false;
            self.title.clone()
        } else {
            None
        }
    }

    fn take_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.responses)
    }

    fn take_agent_events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.agent_events)
    }

    fn take_notifies(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.notifies)
    }

    fn take_progress(&mut self) -> Option<(u8, u8)> {
        if self.progress_changed {
            self.progress_changed = false;
            self.progress
        } else {
            None
        }
    }
}

const AGENT_EVENT_SENTINEL: &str = "panorama://cli-agent";
const OSC777_MAX: usize = 16 * 1024;

const OSC_FG_RESPONSE: &[u8] = b"\x1b]10;rgb:c7c7/d0d0/e0e0\x07";
const OSC_BG_RESPONSE: &[u8] = b"\x1b]11;rgb:0b0b/0e0e/1414\x07";

impl vt100::Callbacks for CwdSink {
    fn unhandled_osc(&mut self, _screen: &mut vt100::Screen, params: &[&[u8]]) {
        if let [b"7", url] = params {
            if let Some(path) = parse_osc7(url) {
                self.prompt_seen = true;
                if self.cwd.as_deref() != Some(path.as_str()) {
                    self.cwd = Some(path);
                    self.changed = true;
                }
            }
        }
        match params {
            [b"10", b"?"] => self.responses.push(OSC_FG_RESPONSE.to_vec()),
            [b"11", b"?"] => self.responses.push(OSC_BG_RESPONSE.to_vec()),
            [b"9", b"4", rest @ ..] => {
                let num = |i: usize| {
                    rest.get(i)
                        .and_then(|s| std::str::from_utf8(s).ok())
                        .and_then(|s| s.parse::<u8>().ok())
                        .unwrap_or(0)
                };
                let next = (num(0), num(1).min(100));
                if self.progress != Some(next) {
                    self.progress = Some(next);
                    self.progress_changed = true;
                }
            }
            [b"9", b"2", msg @ ..] if !msg.is_empty() => {
                let body = msg
                    .iter()
                    .map(|p| String::from_utf8_lossy(p))
                    .collect::<Vec<_>>()
                    .join(";");
                if !body.trim().is_empty() && body.len() <= OSC777_MAX {
                    self.notifies.push((String::new(), body));
                }
            }
            [b"9", msg] if !msg.is_empty() => {
                let body = String::from_utf8_lossy(msg).into_owned();
                if !body.trim().is_empty() && body.len() <= OSC777_MAX {
                    self.notifies.push((String::new(), body));
                }
            }
            _ => {}
        }
        if let [b"777", b"notify", title, rest @ ..] = params {
            if rest.is_empty() {
                return;
            }
            let title = String::from_utf8_lossy(title).into_owned();
            let body = rest
                .iter()
                .map(|p| String::from_utf8_lossy(p))
                .collect::<Vec<_>>()
                .join(";");
            if body.len() > OSC777_MAX {
                return;
            }
            if title == AGENT_EVENT_SENTINEL {
                self.agent_events.push(body);
            } else {
                self.notifies.push((title, body));
            }
        }
    }

    fn set_window_title(&mut self, _screen: &mut vt100::Screen, title: &[u8]) {
        let t = String::from_utf8_lossy(title).trim().to_string();
        if self.title.as_deref() != Some(t.as_str()) {
            self.title = Some(t);
            self.title_changed = true;
        }
    }

    fn copy_to_clipboard(&mut self, _screen: &mut vt100::Screen, _ty: &[u8], data: &[u8]) {
        if let Some(text) = decode_osc52(data) {
            self.clipboard = Some(text);
        }
    }

    fn unhandled_csi(
        &mut self,
        _screen: &mut vt100::Screen,
        i1: Option<u8>,
        _i2: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if i1 != Some(b'?') || (c != 'h' && c != 'l') {
            return;
        }
        if !params.iter().any(|p| **p == [1004]) {
            return;
        }
        let on = c == 'h';
        self.focus_reporting.store(on, Ordering::Relaxed);
        if on {
            let seq: &[u8] = if self.focused.load(Ordering::Relaxed) {
                b"\x1b[I"
            } else {
                b"\x1b[O"
            };
            self.responses.push(seq.to_vec());
        }
    }
}

const OSC52_MAX: usize = 128 * 1024;

fn decode_osc52(data: &[u8]) -> Option<String> {
    if data.is_empty() || data == b"?" || data.len() > OSC52_MAX {
        return None;
    }
    use base64::Engine;
    let cleaned: Vec<u8> = data.iter().copied().filter(|b| !b.is_ascii_whitespace()).collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(&cleaned).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn git_branch(cwd: &str) -> Option<String> {
    let mut dir = Some(Path::new(cwd));
    while let Some(d) = dir {
        let dot_git = d.join(".git");
        if dot_git.is_dir() {
            return branch_from_head(&dot_git.join("HEAD"));
        }
        if dot_git.is_file() {
            let raw = std::fs::read_to_string(&dot_git).ok()?;
            let gitdir = raw.strip_prefix("gitdir:")?.trim();
            let base = if Path::new(gitdir).is_absolute() {
                PathBuf::from(gitdir)
            } else {
                d.join(gitdir)
            };
            return branch_from_head(&base.join("HEAD"));
        }
        dir = d.parent();
    }
    None
}

fn branch_from_head(head: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(head).ok()?;
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("ref: ") {
        return Some(rest.rsplit('/').next().unwrap_or(rest).to_string());
    }
    Some(raw.chars().take(7).collect())
}

fn parse_osc7(url: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(url).ok()?;
    let rest = s.strip_prefix("file://")?;
    let idx = rest.find('/')?;
    let mut path = &rest[idx..];
    let pb = path.as_bytes();
    if pb.len() >= 3 && pb[0] == b'/' && pb[2] == b':' {
        path = &path[1..];
    }
    Some(percent_decode(path))
}

const SCROLLBACK_LINES: usize = 5000;
const RAW_CAP: usize = 256 * 1024;

struct Session {
    tile_id: String,
    shell: String,
    parser: Mutex<vt100::Parser<CwdSink>>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    raw: Mutex<Vec<u8>>,
    dirty: AtomicBool,
    raw_dirty: AtomicBool,
    exited: AtomicBool,
    cols: AtomicU16,
    rows: AtomicU16,
    scrollback: AtomicUsize,
    cwd: Mutex<Option<String>>,
    cwd_dirty: AtomicBool,
    branch: Mutex<Option<String>>,
    cmd: Mutex<Option<(Instant, String)>>,
    clipboard: Mutex<Option<String>>,
    clipboard_dirty: AtomicBool,
    title: Mutex<Option<String>>,
    title_dirty: AtomicBool,
    events: Mutex<Vec<String>>,
    focused: Arc<AtomicBool>,
    focus_reporting: Arc<AtomicBool>,
}

fn buffer_path(tile_id: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    let dir = Path::new(&home).join(".panorama").join("pty-buffers");
    std::fs::create_dir_all(&dir).ok()?;
    let mut name = String::with_capacity(tile_id.len());
    for ch in tile_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            name.push(ch);
        } else {
            name.push('_');
        }
    }
    Some(dir.join(format!("{name}.raw")))
}

fn load_buffer(tile_id: &str) -> Vec<u8> {
    buffer_path(tile_id)
        .and_then(|p| std::fs::read(p).ok())
        .unwrap_or_default()
}

fn flush_session(s: &Session) {
    if !s.raw_dirty.swap(false, Ordering::Relaxed) {
        return;
    }
    let data = s.raw.lock().map(|r| r.clone()).unwrap_or_default();
    if let Some(p) = buffer_path(&s.tile_id) {
        let _ = std::fs::write(p, &data);
    }
}

fn panorama_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(Path::new(&home).join(".panorama"))
}

fn sanitize_key(key: &str) -> String {
    let mut name = String::with_capacity(key.len());
    for ch in key.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            name.push(ch);
        } else {
            name.push('_');
        }
    }
    name
}

fn binding_path(tile_id: &str) -> Option<std::path::PathBuf> {
    let dir = panorama_dir()?.join("agent-bindings");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(format!("{}.json", sanitize_key(tile_id))))
}

fn tile_for_session(session_id: &str) -> Option<String> {
    let dir = panorama_dir()?.join("agent-bindings");
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let Ok(raw) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else {
            continue;
        };
        if v.get("agentSessionId").and_then(|s| s.as_str()) == Some(session_id) {
            if let Some(tile) = v.get("tileId").and_then(|t| t.as_str()) {
                return Some(tile.to_string());
            }
        }
    }
    None
}

fn read_binding(tile_id: &str) -> Option<String> {
    let raw = std::fs::read_to_string(binding_path(tile_id)?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("agentSessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

fn read_binding_rec(tile_id: &str) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(binding_path(tile_id)?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn claude_home() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())?;
    Some(Path::new(&home).join(".claude"))
}

fn settings_field(key: &str) -> Option<String> {
    let raw = std::fs::read_to_string(claude_home()?.join("settings.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get(key)
        .and_then(|m| m.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn default_model() -> Option<String> {
    settings_field("model")
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

fn status_from_session_json(raw: &str, agent_id: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw).ok()?;
    if v.get("sessionId").and_then(|s| s.as_str()) != Some(agent_id) {
        return None;
    }
    v.get("status").and_then(|s| s.as_str()).map(|s| s.to_string())
}

fn read_session_status(agent_id: &str, cached: &mut Option<PathBuf>) -> Option<String> {
    let matches = |path: &Path| -> Option<String> {
        let raw = std::fs::read_to_string(path).ok()?;
        status_from_session_json(&raw, agent_id)
    };
    if let Some(path) = cached.clone() {
        if let Some(status) = matches(&path) {
            return Some(status);
        }
        *cached = None;
    }
    let dir = claude_home()?.join("sessions");
    for entry in std::fs::read_dir(&dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        if let Some(status) = matches(&path) {
            *cached = Some(path);
            return Some(status);
        }
    }
    None
}

fn usage_tokens(usage: &serde_json::Value) -> Option<u64> {
    let get = |k: &str| usage.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
    let sum = get("input_tokens") + get("cache_read_input_tokens") + get("cache_creation_input_tokens");
    (sum > 0).then_some(sum)
}

const TRANSCRIPT_SEED_CAP: u64 = 256 * 1024;

#[derive(Default)]
struct ClaudeTracker {
    agent_id: Option<String>,
    cwd: Option<String>,
    path: Option<PathBuf>,
    offset: u64,
    model: Option<String>,
    mode: Option<String>,
    perm: Option<String>,
    tokens: Option<u64>,
    effort: Option<String>,
    default_model: Option<String>,
    status: Option<String>,
    status_path: Option<PathBuf>,
    sent: Option<String>,
    reset: bool,
}

impl ClaudeTracker {
    fn ingest(&mut self, line: &str) {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            return;
        };
        match entry.get("type").and_then(|t| t.as_str()) {
            Some("mode") => {
                if let Some(m) = entry.get("mode").and_then(|m| m.as_str()) {
                    self.mode = Some(m.to_string());
                }
            }
            Some("permission-mode") => {
                if let Some(p) = entry.get("permissionMode").and_then(|p| p.as_str()) {
                    self.perm = Some(p.to_string());
                }
            }
            _ => {}
        }
        let msg = entry.get("message");
        if msg.and_then(|m| m.get("role")).and_then(|r| r.as_str()) != Some("assistant") {
            return;
        }
        if let Some(model) = msg.and_then(|m| m.get("model")).and_then(|m| m.as_str()) {
            self.model = Some(model.to_string());
        }
        if let Some(tok) = msg.and_then(|m| m.get("usage")).and_then(usage_tokens) {
            self.tokens = Some(tok);
        }
    }

    fn tail(&mut self) {
        let Some(path) = self.path.clone() else { return };
        let Ok(size) = std::fs::metadata(&path).map(|m| m.len()) else {
            return;
        };
        if size <= self.offset {
            if size < self.offset {
                self.offset = 0;
            }
            return;
        }
        let start = if self.offset == 0 && size > TRANSCRIPT_SEED_CAP {
            size - TRANSCRIPT_SEED_CAP
        } else {
            self.offset
        };
        let Ok(mut file) = std::fs::File::open(&path) else {
            return;
        };
        if file.seek(SeekFrom::Start(start)).is_err() {
            return;
        }
        let mut buf = String::new();
        if file.read_to_string(&mut buf).is_err() {
            return;
        }
        self.offset = size;
        for line in buf.split('\n') {
            let line = line.trim();
            if !line.is_empty() {
                self.ingest(line);
            }
        }
    }

    fn poll(&mut self, tile_id: &str) -> Option<String> {
        let rec = read_binding_rec(tile_id)?;
        let id = rec.get("agentSessionId").and_then(|s| s.as_str())?.to_string();
        if self.agent_id.as_deref() != Some(id.as_str()) {
            let rebind = self.agent_id.is_some();
            *self = ClaudeTracker { default_model: self.default_model.take(), ..Default::default() };
            self.reset = rebind;
            self.cwd = rec.get("cwd").and_then(|s| s.as_str()).map(|s| s.to_string());
            self.agent_id = Some(id);
        }
        if self.path.is_none() {
            let id = self.agent_id.clone()?;
            self.path = find_transcript(&id, self.cwd.as_deref());
        }
        self.tail();
        if let Some(id) = self.agent_id.clone() {
            if let Some(status) = read_session_status(&id, &mut self.status_path) {
                self.status = Some(status);
            }
        }
        if self.default_model.is_none() {
            self.default_model = default_model();
        }
        self.effort = settings_field("effortLevel");
        if self.model.is_none() {
            self.model = self.default_model.clone();
        }

        let mut obj = serde_json::Map::new();
        obj.insert("t".into(), "claude".into());
        if let Some(m) = &self.model {
            obj.insert("model".into(), m.clone().into());
        }
        if let Some(m) = &self.mode {
            obj.insert("mode".into(), m.clone().into());
        }
        if let Some(p) = &self.perm {
            obj.insert("permissionMode".into(), p.clone().into());
        }
        if let Some(t) = self.tokens {
            obj.insert("contextTokens".into(), t.into());
        }
        if let Some(e) = &self.effort {
            obj.insert("effort".into(), e.clone().into());
        }
        if let Some(d) = &self.default_model {
            obj.insert("defaultModel".into(), d.clone().into());
        }
        if let Some(s) = &self.status {
            obj.insert("status".into(), s.clone().into());
        }
        if self.reset {
            obj.insert("reset".into(), true.into());
        }
        if obj.len() == 1 {
            return None;
        }
        let json = serde_json::Value::Object(obj).to_string();
        if self.sent.as_deref() == Some(json.as_str()) {
            return None;
        }
        self.sent = Some(json.clone());
        self.reset = false;
        Some(json)
    }
}

fn agent_event_ws_msg(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let event = v.get("event").and_then(|e| e.as_str())?;
    let field = |k: &str| v.get(k).and_then(|s| s.as_str()).unwrap_or("");
    let msg = serde_json::json!({
        "t": "agentEvent",
        "event": event,
        "sessionId": field("session_id"),
        "project": field("project"),
        "query": field("query"),
        "response": field("response"),
        "toolName": field("tool_name"),
        "message": field("message"),
    });
    Some(msg.to_string())
}

fn status_ws_msg(v: &serde_json::Value) -> Option<String> {
    let mut obj = serde_json::Map::new();
    obj.insert("t".into(), "claude".into());
    let mut put = |k: &str, val: Option<serde_json::Value>| {
        if let Some(val) = val {
            if !val.is_null() {
                obj.insert(k.into(), val);
            }
        }
    };
    put("model", v.pointer("/model/id").cloned());
    put("effort", v.pointer("/effort/level").cloned());
    put("thinking", v.pointer("/thinking/enabled").cloned());
    put("contextTokens", v.pointer("/context_window/total_input_tokens").cloned());
    put("contextPercent", v.pointer("/context_window/used_percentage").cloned());
    put("contextWindow", v.pointer("/context_window/context_window_size").cloned());
    put("costUsd", v.pointer("/cost/total_cost_usd").cloned());
    put("sessionName", v.get("session_name").cloned());
    put("outputStyle", v.pointer("/output_style/name").cloned());
    put("rateFiveHour", v.pointer("/rate_limits/five_hour/used_percentage").cloned());
    put("rateSevenDay", v.pointer("/rate_limits/seven_day/used_percentage").cloned());
    if obj.len() == 1 {
        return None;
    }
    Some(serde_json::Value::Object(obj).to_string())
}

fn handle_agent_status(body: &[u8]) {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else {
        return;
    };
    let Some(session_id) = v.get("session_id").and_then(|s| s.as_str()) else {
        return;
    };
    let Some(tile_id) = tile_for_session(session_id) else {
        return;
    };
    let Some(msg) = status_ws_msg(&v) else {
        return;
    };
    let session = sessions().lock().unwrap().get(&tile_id).cloned();
    if let Some(s) = session {
        s.events.lock().unwrap_or_else(|e| e.into_inner()).push(msg);
    }
}

const STATUS_POST_MAX: usize = 64 * 1024;

fn post_agent_status(input: &str) {
    if input.len() > STATUS_POST_MAX {
        return;
    }
    let addr: std::net::SocketAddr = "127.0.0.1:9777".parse().unwrap();
    let Ok(mut stream) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)) else {
        return;
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(300)));
    let req = format!(
        "POST /agent-status HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        input.len(),
        input
    );
    let _ = stream.write_all(req.as_bytes());
}

fn statusline_chain_path() -> Option<PathBuf> {
    Some(panorama_dir()?.join("statusline-chain.json"))
}

fn load_statusline_chain() -> Option<String> {
    let raw = std::fs::read_to_string(statusline_chain_path()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let cmd = v.get("command").and_then(|c| c.as_str())?.trim().to_string();
    if cmd.is_empty() || cmd.ends_with("\" statusline") || cmd.ends_with(" statusline") {
        return None;
    }
    Some(cmd)
}

fn run_chained_statusline(command: &str, input: &str) -> Option<String> {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new(resolve_powershell());
        c.args(["-NoProfile", "-NonInteractive", "-Command", command]);
        c
    } else {
        let mut c = std::process::Command::new("sh");
        c.args(["-c", command]);
        c
    };
    let mut child = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_string())
}

fn default_status_line(v: &serde_json::Value) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = v.pointer("/model/display_name").and_then(|s| s.as_str()) {
        parts.push(m.to_string());
    }
    if let Some(p) = v.pointer("/context_window/used_percentage").and_then(|n| n.as_f64()) {
        parts.push(format!("{}% ctx", p.round() as i64));
    }
    if let Some(c) = v.pointer("/cost/total_cost_usd").and_then(|n| n.as_f64()) {
        parts.push(format!("${c:.2}"));
    }
    if parts.is_empty() {
        parts.push("Claude".to_string());
    }
    parts.join(" \u{00b7} ")
}

fn statusline_cmd() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    post_agent_status(&input);
    if let Some(chain) = load_statusline_chain() {
        if let Some(line) = run_chained_statusline(&chain, &input) {
            println!("{line}");
            return;
        }
    }
    let v: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
    println!("{}", default_status_line(&v));
}

const EMIT_EVENT_MIN_CC: (u64, u64, u64) = (2, 1, 141);
const EVENT_TEXT_CAP: usize = 200;

fn cc_supports_terminal_sequence() -> bool {
    let Ok(raw) = std::env::var("CLAUDE_CODE_VERSION") else {
        return true;
    };
    let mut parts = raw
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u64>().ok());
    let ver = (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    );
    ver >= EMIT_EVENT_MIN_CC
}

fn sanitize_event_text(raw: &str) -> String {
    let clean: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() > EVENT_TEXT_CAP {
        let cut: String = clean.chars().take(EVENT_TEXT_CAP - 3).collect();
        format!("{cut}...")
    } else {
        clean
    }
}

fn transcript_last_texts(path: &str) -> (String, String) {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return (String::new(), String::new());
    };
    let mut query = String::new();
    let mut response = String::new();
    for line in raw.lines() {
        let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let kind = entry.get("type").and_then(|t| t.as_str());
        let content = entry.get("message").and_then(|m| m.get("content"));
        let text = match content {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(serde_json::Value::Array(blocks)) => blocks
                .iter()
                .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join(" "),
            _ => continue,
        };
        if text.trim().is_empty() {
            continue;
        }
        match kind {
            Some("user") => query = text,
            Some("assistant") => response = text,
            _ => {}
        }
    }
    (sanitize_event_text(&query), sanitize_event_text(&response))
}

fn emit_event(event: &str) {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let evt: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);

    if std::env::var("PANORAMA_TILE_ID").is_err() {
        return;
    }
    if !cc_supports_terminal_sequence() {
        return;
    }
    if event == "stop"
        && evt
            .get("stop_hook_active")
            .and_then(|b| b.as_bool())
            .unwrap_or(false)
    {
        return;
    }

    let field = |k: &str| evt.get(k).and_then(|s| s.as_str()).unwrap_or("").to_string();
    let cwd = field("cwd");
    let project = Path::new(&cwd)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    let mut payload = serde_json::json!({
        "v": 1,
        "agent": "claude",
        "event": event,
        "session_id": field("session_id"),
        "cwd": cwd,
        "project": project,
    });
    let obj = payload.as_object_mut().unwrap();
    match event {
        "stop" => {
            std::thread::sleep(Duration::from_millis(300));
            let (query, response) = transcript_last_texts(&field("transcript_path"));
            obj.insert("query".into(), query.into());
            obj.insert("response".into(), response.into());
        }
        "permission" => {
            obj.insert("tool_name".into(), field("tool_name").into());
            let preview = evt
                .get("tool_input")
                .map(|t| sanitize_event_text(&t.to_string()))
                .unwrap_or_default();
            obj.insert("message".into(), preview.into());
        }
        "notification" => {
            obj.insert("message".into(), sanitize_event_text(&field("message")).into());
        }
        "prompt-submit" => {
            obj.insert("query".into(), sanitize_event_text(&field("prompt")).into());
        }
        _ => {}
    }

    let body = payload.to_string();
    let seq = format!("\x1b]777;notify;{AGENT_EVENT_SENTINEL};{body}\x07");
    let out = serde_json::json!({ "terminalSequence": seq });
    println!("{out}");
}

fn record_agent() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let evt: serde_json::Value = serde_json::from_str(&input).unwrap_or(serde_json::Value::Null);
    let session_id = evt.get("session_id").and_then(|s| s.as_str());
    let tile_id = std::env::var("PANORAMA_TILE_ID").ok();
    if let (Some(session_id), Some(tile_id)) = (session_id, tile_id) {
        if let Some(path) = binding_path(&tile_id) {
            let cwd = evt.get("cwd").and_then(|s| s.as_str());
            let rec = serde_json::json!({
                "agentSessionId": session_id,
                "cwd": cwd,
                "tileId": tile_id,
            });
            let _ = std::fs::write(path, rec.to_string());
        }
    }
}

fn install_claude_hook() {
    let Some(home) = std::env::var("USERPROFILE")
        .ok()
        .or_else(|| std::env::var("HOME").ok())
    else {
        return;
    };
    let claude_dir = Path::new(&home).join(".claude");
    if !claude_dir.exists() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let file = claude_dir.join("settings.json");

    let mut json: serde_json::Value = std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !json.is_object() {
        json = serde_json::json!({});
    }

    let hooks = json
        .as_object_mut()
        .unwrap()
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}));
    if !hooks.is_object() {
        *hooks = serde_json::json!({});
    }
    let hooks = hooks.as_object_mut().unwrap();

    let exe = exe.display();
    let entries: &[(&str, Option<&str>, String, &str)] = &[
        ("SessionStart", None, format!("\"{exe}\" record-agent"), "record-agent"),
        ("Stop", None, format!("\"{exe}\" emit-event stop"), "emit-event"),
        ("PermissionRequest", None, format!("\"{exe}\" emit-event permission"), "emit-event"),
        ("Notification", Some("idle_prompt"), format!("\"{exe}\" emit-event notification"), "emit-event"),
        ("UserPromptSubmit", None, format!("\"{exe}\" emit-event prompt-submit"), "emit-event"),
    ];

    for (name, matcher, command, marker) in entries {
        let existing = hooks.get(*name).and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let mut cleaned: Vec<serde_json::Value> = existing
            .into_iter()
            .filter(|entry| {
                !entry
                    .get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|arr| {
                        arr.iter().any(|h| {
                            h.get("command")
                                .and_then(|c| c.as_str())
                                .map(|c| c.contains(marker))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .collect();
        let mut entry = serde_json::json!({
            "hooks": [ { "type": "command", "command": command } ]
        });
        if let Some(m) = matcher {
            entry.as_object_mut().unwrap().insert("matcher".into(), (*m).into());
        }
        cleaned.push(entry);
        hooks.insert((*name).into(), serde_json::Value::Array(cleaned));
    }

    let our_status = format!("\"{exe}\" statusline");
    let current = json
        .get("statusLine")
        .and_then(|s| s.get("command"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string());
    if current.as_deref() != Some(our_status.as_str()) {
        if let Some(prev) = current {
            let trimmed = prev.trim();
            if !trimmed.is_empty() && !trimmed.ends_with(" statusline") {
                if let Some(path) = statusline_chain_path() {
                    let _ = std::fs::write(path, serde_json::json!({ "command": prev }).to_string());
                }
            }
        }
        json.as_object_mut().unwrap().insert(
            "statusLine".into(),
            serde_json::json!({ "type": "command", "command": our_status }),
        );
    }

    if let Ok(text) = serde_json::to_string_pretty(&json) {
        let _ = std::fs::write(&file, text);
    }
}

fn utf8_valid_len(data: &[u8]) -> usize {
    match std::str::from_utf8(data) {
        Ok(_) => data.len(),
        Err(e) if e.error_len().is_none() => e.valid_up_to(),
        Err(_) => data.len(),
    }
}

fn display_command_name(raw: &str) -> String {
    let raw = raw.trim();
    let base = Path::new(raw)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(raw);
    let lower = base.to_ascii_lowercase();
    if let Some(stripped) = lower.strip_suffix(".exe") {
        base[..stripped.len()].to_string()
    } else {
        base.to_string()
    }
}

#[cfg(windows)]
fn foreground_command(pid: u32) -> Option<String> {
    let script = format!(
        "$c = Get-CimInstance Win32_Process -Filter \"ParentProcessId = {pid}\" | Sort-Object ProcessId; if ($c) {{ ($c | Select-Object -Last 1).Name }}"
    );
    let out = hidden_command(&resolve_powershell())
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(display_command_name(&name))
    }
}

#[cfg(unix)]
fn foreground_command(pid: u32) -> Option<String> {
    let out = hidden_command("ps")
        .args(["-o", "comm=", "-g", &pid.to_string()])
        .stdin(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let last = text.lines().filter(|l| !l.trim().is_empty()).last()?;
    Some(display_command_name(last))
}

fn capture_text(raw: &[u8], cols: u16, lines: usize) -> String {
    let rows = lines.clamp(1, SCROLLBACK_LINES) as u16;
    let cols = cols.max(2);
    let mut p = vt100::Parser::new(rows, cols, 0);
    p.process(raw);
    let screen = p.screen();
    let mut out: Vec<String> = Vec::with_capacity(rows as usize);
    for r in 0..rows {
        let mut line = String::new();
        for c in 0..cols {
            match screen.cell(r, c) {
                Some(cell) if !cell.contents().is_empty() => line.push_str(cell.contents()),
                _ => line.push(' '),
            }
        }
        while line.ends_with(' ') {
            line.pop();
        }
        out.push(line);
    }
    while out.last().map(|l| l.is_empty()).unwrap_or(false) {
        out.pop();
    }
    out.join("\n")
}

fn sessions() -> &'static Mutex<HashMap<String, Arc<Session>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Session>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone)]
struct Params {
    tile_id: String,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    shell: Option<String>,
    target: Option<String>,
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
    }
}

#[cfg(windows)]
fn hidden_command(program: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new(program);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(not(windows))]
fn hidden_command(program: &str) -> std::process::Command {
    std::process::Command::new(program)
}

fn command_exists(cmd: &str) -> bool {
    let finder = if cfg!(windows) { "where.exe" } else { "which" };
    hidden_command(finder)
        .arg(cmd)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn resolve_powershell() -> String {
    static CACHE: OnceLock<String> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            if command_exists("pwsh.exe") {
                "pwsh.exe".into()
            } else {
                "powershell.exe".into()
            }
        })
        .clone()
}

fn resolve_target(target: Option<&str>) -> Option<String> {
    match target {
        None | Some("auto") | Some("") => {
            if cfg!(windows) {
                Some(resolve_powershell())
            } else {
                None
            }
        }
        Some("powershell") => Some(resolve_powershell()),
        _ => None,
    }
}

fn powershell_osc7_snippet() -> String {
    [
        "",
        "if ($env:PANORAMA_TERMINAL) {",
        "    $Global:__panoOldPrompt = $function:prompt",
        "    function prompt {",
        r"        $p = (Get-Location).ProviderPath -replace '\\', '/'",
        r#"        [Console]::Write("$([char]27)]7;file://$env:COMPUTERNAME/$p$([char]7)")"#,
        "        & $Global:__panoOldPrompt",
        "    }",
        "}",
        "",
    ]
    .join("\r\n")
}

fn ensure_powershell_profile_osc7(shell: &str) {
    if !cfg!(windows) {
        return;
    }
    static DONE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let done = DONE.get_or_init(|| Mutex::new(HashSet::new()));
    {
        let mut d = done.lock().unwrap();
        if d.contains(shell) {
            return;
        }
        d.insert(shell.to_string());
    }
    let shell = shell.to_string();
    std::thread::spawn(move || {
        let out = hidden_command(&shell)
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$PROFILE.CurrentUserCurrentHost",
            ])
            .output();
        let out = match out {
            Ok(o) => o,
            Err(_) => return,
        };
        let profile_path = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if profile_path.is_empty() {
            return;
        }
        let existing = std::fs::read_to_string(&profile_path).unwrap_or_default();
        if existing.contains("__panoOldPrompt") {
            return;
        }
        if let Some(parent) = Path::new(&profile_path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&profile_path)
        {
            let _ = f.write_all(powershell_osc7_snippet().as_bytes());
        }
    });
}

fn osc7_shell_hook(shell: &str) -> Option<String> {
    let base = Path::new(shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if base == "zsh" {
        return Some(
            r#" __pano_osc7() { printf "\e]7;file://%s%s\a" "$HOST" "$PWD"; }; precmd_functions+=(__pano_osc7); clear"#
                .to_string(),
        );
    }
    if base == "bash" || base == "sh" {
        return Some(
            r#" PROMPT_COMMAND='printf "\e]7;file://%s%s\a" "$HOSTNAME" "$PWD"'${PROMPT_COMMAND:+";$PROMPT_COMMAND"}; clear"#
                .to_string(),
        );
    }
    None
}

fn inject_osc7(session: &Arc<Session>, shell: &str) {
    let base = Path::new(shell)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if base.starts_with("powershell") || base.starts_with("pwsh") {
        ensure_powershell_profile_osc7(shell);
        return;
    }
    if let Some(hook) = osc7_shell_hook(shell) {
        let s = session.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            if s.exited.load(Ordering::Relaxed) {
                return;
            }
            if let Ok(mut w) = s.writer.lock() {
                let _ = w.write_all(hook.as_bytes());
                let _ = w.write_all(b"\n");
                let _ = w.flush();
            }
        });
    }
}

fn spawn_session(
    p: &Params,
    permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<Arc<Session>, String> {
    let shell = resolve_target(p.target.as_deref())
        .or_else(|| p.shell.clone())
        .unwrap_or_else(default_shell);
    let cols = p.cols.max(2);
    let rows = p.rows.max(2);
    let cwd = p
        .cwd
        .clone()
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = CommandBuilder::new(&shell);
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("PANORAMA_TERMINAL", "1");
    cmd.env("PANORAMA_TILE_ID", &p.tile_id);

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn '{shell}': {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("reader: {e}"))?;
    let writer = pair.master.take_writer().map_err(|e| format!("writer: {e}"))?;
    drop(pair.slave);

    let seed = load_buffer(&p.tile_id);
    let focused = Arc::new(AtomicBool::new(true));
    let focus_reporting = Arc::new(AtomicBool::new(false));
    let sink = CwdSink {
        focused: focused.clone(),
        focus_reporting: focus_reporting.clone(),
        ..Default::default()
    };
    let mut parser = vt100::Parser::new_with_callbacks(rows, cols, SCROLLBACK_LINES, sink);
    if !seed.is_empty() {
        parser.process(&seed);
        let _ = parser.callbacks_mut().take_clipboard();
        let _ = parser.callbacks_mut().take_responses();
        let _ = parser.callbacks_mut().take_agent_events();
        let _ = parser.callbacks_mut().take_notifies();
        let _ = parser.callbacks_mut().take_progress();
    }

    let session = Arc::new(Session {
        tile_id: p.tile_id.clone(),
        shell: shell.clone(),
        parser: Mutex::new(parser),
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child: Mutex::new(child),
        raw: Mutex::new(seed),
        dirty: AtomicBool::new(true),
        raw_dirty: AtomicBool::new(false),
        exited: AtomicBool::new(false),
        cols: AtomicU16::new(cols),
        rows: AtomicU16::new(rows),
        scrollback: AtomicUsize::new(0),
        cwd: Mutex::new(Some(cwd.clone())),
        cwd_dirty: AtomicBool::new(true),
        branch: Mutex::new(git_branch(&cwd)),
        cmd: Mutex::new(None),
        clipboard: Mutex::new(None),
        clipboard_dirty: AtomicBool::new(false),
        title: Mutex::new(None),
        title_dirty: AtomicBool::new(false),
        events: Mutex::new(Vec::new()),
        focused,
        focus_reporting,
    });

    let s = session.clone();
    std::thread::spawn(move || {
        let mut permit = Some(permit);
        let mut reader = reader;
        let mut buf = [0u8; 8192];
        let mut carry: Vec<u8> = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    permit.take();
                    let mut data = Vec::with_capacity(carry.len() + n);
                    data.append(&mut carry);
                    data.extend_from_slice(&buf[..n]);
                    let valid = utf8_valid_len(&data);
                    if valid < data.len() {
                        carry.extend_from_slice(&data[valid..]);
                    }
                    let chunk = &data[..valid];
                    if chunk.is_empty() {
                        continue;
                    }
                    let responses = {
                        let mut p = s.parser.lock().unwrap_or_else(|e| e.into_inner());
                        p.process(chunk);
                        if let Some(new_cwd) = p.callbacks_mut().take_changed() {
                            *s.cwd.lock().unwrap_or_else(|e| e.into_inner()) = Some(new_cwd);
                            s.cwd_dirty.store(true, Ordering::Relaxed);
                        }
                        if let Some(prompt_cwd) = p.callbacks_mut().take_prompt() {
                            let branch = git_branch(&prompt_cwd);
                            {
                                let mut cur = s.branch.lock().unwrap_or_else(|e| e.into_inner());
                                if *cur != branch {
                                    *cur = branch;
                                    s.cwd_dirty.store(true, Ordering::Relaxed);
                                }
                            }
                            let done = s.cmd.lock().unwrap_or_else(|e| e.into_inner()).take();
                            if let Some((start, cmd)) = done {
                                let secs = start.elapsed().as_secs();
                                if secs >= LONG_CMD_SECS {
                                    let msg = serde_json::json!({
                                        "t": "notify",
                                        "title": cmd,
                                        "body": format!("Finished in {}", fmt_duration(secs)),
                                    });
                                    s.events
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner())
                                        .push(msg.to_string());
                                }
                            }
                        }
                        if let Some(text) = p.callbacks_mut().take_clipboard() {
                            *s.clipboard.lock().unwrap_or_else(|e| e.into_inner()) = Some(text);
                            s.clipboard_dirty.store(true, Ordering::Relaxed);
                        }
                        if let Some(title) = p.callbacks_mut().take_title() {
                            if title != s.shell {
                                *s.title.lock().unwrap_or_else(|e| e.into_inner()) = Some(title);
                                s.title_dirty.store(true, Ordering::Relaxed);
                            }
                        }
                        let mut outgoing: Vec<String> = Vec::new();
                        for body in p.callbacks_mut().take_agent_events() {
                            if let Some(msg) = agent_event_ws_msg(&body) {
                                outgoing.push(msg);
                            }
                        }
                        for (title, body) in p.callbacks_mut().take_notifies() {
                            let msg = serde_json::json!({ "t": "notify", "title": title, "body": body });
                            outgoing.push(msg.to_string());
                        }
                        if let Some((state, pct)) = p.callbacks_mut().take_progress() {
                            let msg = serde_json::json!({ "t": "progress", "state": state, "pct": pct });
                            outgoing.push(msg.to_string());
                        }
                        if !outgoing.is_empty() {
                            s.events
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .extend(outgoing);
                        }
                        p.callbacks_mut().take_responses()
                    };
                    if !responses.is_empty() {
                        if let Ok(mut w) = s.writer.lock() {
                            for resp in &responses {
                                let _ = w.write_all(resp);
                            }
                            let _ = w.flush();
                        }
                    }
                    if let Ok(mut raw) = s.raw.lock() {
                        raw.extend_from_slice(chunk);
                        if raw.len() > RAW_CAP {
                            let cut = raw.len() - RAW_CAP;
                            raw.drain(0..cut);
                        }
                    }
                    s.raw_dirty.store(true, Ordering::Relaxed);
                    s.dirty.store(true, Ordering::Relaxed);
                }
            }
        }
        s.exited.store(true, Ordering::Relaxed);
        s.dirty.store(true, Ordering::Relaxed);
        flush_session(&s);
        sessions().lock().unwrap().remove(&s.tile_id);
    });

    inject_osc7(&session, &shell);
    Ok(session)
}

const MAX_CONCURRENT_BOOT: usize = 3;

fn spawn_sem() -> Arc<tokio::sync::Semaphore> {
    static SEM: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    SEM.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_BOOT)))
        .clone()
}

fn lookup_live(tile_id: &str) -> Option<Arc<Session>> {
    let map = sessions().lock().unwrap();
    map.get(tile_id)
        .filter(|s| !s.exited.load(Ordering::Relaxed))
        .cloned()
}

async fn get_or_create(p: &Params) -> Result<(Arc<Session>, bool), String> {
    if let Some(s) = lookup_live(&p.tile_id) {
        return Ok((s, true));
    }
    let permit = spawn_sem()
        .acquire_owned()
        .await
        .map_err(|e| e.to_string())?;
    if let Some(s) = lookup_live(&p.tile_id) {
        return Ok((s, true));
    }
    let params = p.clone();
    let s = tokio::task::spawn_blocking(move || spawn_session(&params, permit))
        .await
        .map_err(|e| format!("spawn task: {e}"))??;
    sessions().lock().unwrap().insert(p.tile_id.clone(), s.clone());
    Ok((s, false))
}

fn resize_session(s: &Session, cols: u16, rows: u16) {
    let cols = cols.max(2);
    let rows = rows.max(2);
    if s.cols.load(Ordering::Relaxed) == cols && s.rows.load(Ordering::Relaxed) == rows {
        return;
    }
    s.cols.store(cols, Ordering::Relaxed);
    s.rows.store(rows, Ordering::Relaxed);
    if let Ok(master) = s.master.lock() {
        let _ = master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        });
    }
    s.parser
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .screen_mut()
        .set_size(rows, cols);
    s.dirty.store(true, Ordering::Relaxed);
}

fn scroll_session(s: &Session, dir: i64, lines: usize, col: usize, row: usize) {
    let (alt, app_cursor, mouse, enc) = {
        let parser = s.parser.lock().unwrap_or_else(|e| e.into_inner());
        let sc = parser.screen();
        (
            sc.alternate_screen(),
            sc.application_cursor(),
            sc.mouse_protocol_mode(),
            sc.mouse_protocol_encoding(),
        )
    };

    if mouse != vt100::MouseProtocolMode::None {
        let cb: i64 = if dir > 0 { 64 } else { 65 };
        let mut seq: Vec<u8> = Vec::new();
        match enc {
            vt100::MouseProtocolEncoding::Sgr => {
                seq.extend_from_slice(format!("\x1b[<{};{};{}M", cb, col, row).as_bytes());
            }
            _ => {
                let cx = (col.min(223) as u8).saturating_add(32);
                let cy = (row.min(223) as u8).saturating_add(32);
                seq.extend_from_slice(&[0x1b, b'[', b'M', (cb as u8) + 32, cx, cy]);
            }
        }
        if let Ok(mut w) = s.writer.lock() {
            for _ in 0..lines.max(1) {
                let _ = w.write_all(&seq);
            }
            let _ = w.flush();
        }
        return;
    }

    if alt {
        let arrow: &[u8] = match (dir > 0, app_cursor) {
            (true, true) => b"\x1bOA",
            (true, false) => b"\x1b[A",
            (false, true) => b"\x1bOB",
            (false, false) => b"\x1b[B",
        };
        if let Ok(mut w) = s.writer.lock() {
            for _ in 0..lines {
                let _ = w.write_all(arrow);
            }
            let _ = w.flush();
        }
        return;
    }

    let cur = s.scrollback.load(Ordering::Relaxed) as i64;
    let desired = (cur + dir * lines as i64).max(0) as usize;
    let actual = {
        let mut parser = s.parser.lock().unwrap_or_else(|e| e.into_inner());
        parser.screen_mut().set_scrollback(desired);
        parser.screen().scrollback()
    };
    s.scrollback.store(actual, Ordering::Relaxed);
    s.dirty.store(true, Ordering::Relaxed);
}

fn mouse_session(s: &Session, kind: u8, button: i64, col: usize, row: usize, mods: i64) {
    let (mouse, enc) = {
        let parser = s.parser.lock().unwrap_or_else(|e| e.into_inner());
        let sc = parser.screen();
        (sc.mouse_protocol_mode(), sc.mouse_protocol_encoding())
    };
    if mouse == vt100::MouseProtocolMode::None {
        return;
    }
    let motion = kind == 2;
    if motion && button == 3 && mouse != vt100::MouseProtocolMode::AnyMotion {
        return;
    }
    if motion
        && button != 3
        && mouse != vt100::MouseProtocolMode::ButtonMotion
        && mouse != vt100::MouseProtocolMode::AnyMotion
    {
        return;
    }
    let release = kind == 1;
    if release && mouse == vt100::MouseProtocolMode::Press {
        return;
    }
    let mut cb = button + mods;
    if motion {
        cb += 32;
    }
    let mut seq: Vec<u8> = Vec::new();
    match enc {
        vt100::MouseProtocolEncoding::Sgr => {
            let term = if release { 'm' } else { 'M' };
            seq.extend_from_slice(format!("\x1b[<{};{};{}{}", cb, col, row, term).as_bytes());
        }
        _ => {
            let base = if release { 3 + mods } else { cb };
            let cx = (col.min(223) as u8).saturating_add(32);
            let cy = (row.min(223) as u8).saturating_add(32);
            seq.extend_from_slice(&[0x1b, b'[', b'M', (base as u8).saturating_add(32), cx, cy]);
        }
    }
    if let Ok(mut w) = s.writer.lock() {
        let _ = w.write_all(&seq);
        let _ = w.flush();
    }
}

fn focus_session(s: &Session, focused: bool) {
    let prev = s.focused.swap(focused, Ordering::Relaxed);
    if prev == focused || !s.focus_reporting.load(Ordering::Relaxed) {
        return;
    }
    if let Ok(mut w) = s.writer.lock() {
        let _ = w.write_all(if focused { b"\x1b[I" } else { b"\x1b[O" });
        let _ = w.flush();
    }
}

fn kill_session(s: &Arc<Session>) {
    let s = s.clone();
    std::thread::spawn(move || {
        let pid = s.child.lock().ok().and_then(|c| c.process_id());
        #[cfg(windows)]
        {
            if let Some(pid) = pid {
                let _ = hidden_command("taskkill.exe")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status();
            }
        }
        #[cfg(unix)]
        {
            if let Some(pid) = pid {
                unsafe {
                    libc::killpg(pid as i32, libc::SIGTERM);
                }
                std::thread::sleep(Duration::from_millis(150));
                unsafe {
                    libc::killpg(pid as i32, libc::SIGKILL);
                }
            }
        }
        if let Ok(mut c) = s.child.lock() {
            let _ = c.kill();
        }
        s.exited.store(true, Ordering::Relaxed);
        flush_session(&s);
        sessions().lock().unwrap().remove(&s.tile_id);
    });
}

fn build_frame(s: &Session) -> Vec<u8> {
    let mut parser = s.parser.lock().unwrap_or_else(|e| e.into_inner());
    parser
        .screen_mut()
        .set_scrollback(s.scrollback.load(Ordering::Relaxed));
    let screen = parser.screen();
    let offset = screen.scrollback() as u16;
    let (rows, cols) = screen.size();

    let mut text = String::with_capacity(rows as usize * (cols as usize + 1));
    let mut attrs: Vec<u32> = Vec::with_capacity(rows as usize * cols as usize * 2);
    for r in 0..rows {
        for c in 0..cols {
            let cell = screen.cell(r, c);
            match cell {
                Some(cell) if !cell.contents().is_empty() => text.push_str(cell.contents()),
                _ => text.push(' '),
            }
            let (mut fg, mut bg, mut flags) = (DEFAULT_FG, None::<u32>, 0u32);
            if let Some(cell) = cell {
                fg = color_rgb(cell.fgcolor()).unwrap_or(DEFAULT_FG);
                bg = color_rgb(cell.bgcolor());
                if cell.bold() {
                    flags |= 1;
                }
                if cell.italic() {
                    flags |= 2;
                }
                if cell.underline() {
                    flags |= 4;
                }
                if cell.inverse() {
                    let real_bg = bg.unwrap_or(DEFAULT_BG);
                    bg = Some(fg);
                    fg = real_bg;
                }
                if cell.dim() {
                    fg = blend(fg, bg.unwrap_or(DEFAULT_BG), DIM_MIX);
                }
            }
            attrs.push((fg & 0x00FF_FFFF) | (flags << 24));
            attrs.push(match bg {
                Some(rgb) => (rgb & 0x00FF_FFFF) | 0x8000_0000,
                None => 0,
            });
        }
        if r + 1 < rows {
            text.push('\n');
        }
    }

    let (cur_r, cur_c) = screen.cursor_position();
    let cursor = ((cur_r as u32) << 16) | (cur_c as u32);
    let hidden = screen.hide_cursor();
    let mouse_byte: u8 = match screen.mouse_protocol_mode() {
        vt100::MouseProtocolMode::None => 0,
        vt100::MouseProtocolMode::Press => 1,
        vt100::MouseProtocolMode::PressRelease => 2,
        vt100::MouseProtocolMode::ButtonMotion => 3,
        vt100::MouseProtocolMode::AnyMotion => 4,
    };
    drop(parser);

    let text_bytes = text.as_bytes();
    let mut buf = Vec::with_capacity(16 + text_bytes.len() + attrs.len() * 4);
    buf.push(1u8);
    buf.extend_from_slice(&rows.to_le_bytes());
    buf.extend_from_slice(&cols.to_le_bytes());
    buf.extend_from_slice(&cursor.to_le_bytes());
    buf.push(if hidden { 1 } else { 0 });
    buf.push(mouse_byte);
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&(text_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(text_bytes);
    for w in &attrs {
        buf.extend_from_slice(&w.to_le_bytes());
    }
    buf
}

const LONG_CMD_SECS: u64 = 10;

fn fmt_duration(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}h{:02}m", secs / 3600, (secs % 3600) / 60)
    } else if secs >= 60 {
        format!("{}m{:02}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}

fn current_command_line(s: &Arc<Session>) -> Option<String> {
    let parser = s.parser.lock().unwrap_or_else(|e| e.into_inner());
    let screen = parser.screen();
    let (row, _) = screen.cursor_position();
    let cols = screen.size().1;
    let mut line = String::new();
    for c in 0..cols {
        match screen.cell(row, c) {
            Some(cell) if !cell.contents().is_empty() => line.push_str(cell.contents()),
            _ => line.push(' '),
        }
    }
    let line = line.trim();
    let cmd = match line.rsplit_once(['>', '$', '#', '\u{276f}']) {
        Some((_, tail)) => tail.trim(),
        None => line,
    };
    if cmd.is_empty() || cmd == "claude" || cmd.starts_with("claude ") {
        None
    } else {
        Some(cmd.chars().take(60).collect())
    }
}

fn handle_client_msg(s: &Arc<Session>, text: &str) {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return,
    };
    match v.get("t").and_then(|t| t.as_str()) {
        Some("in") => {
            if let Some(d) = v.get("d").and_then(|d| d.as_str()) {
                s.scrollback.store(0, Ordering::Relaxed);
                s.dirty.store(true, Ordering::Relaxed);
                if d.contains('\r') || d.contains('\n') {
                    let line = current_command_line(s);
                    let mut cmd = s.cmd.lock().unwrap_or_else(|e| e.into_inner());
                    *cmd = line.map(|l| (Instant::now(), l));
                }
                if let Ok(mut w) = s.writer.lock() {
                    let _ = w.write_all(d.as_bytes());
                    let _ = w.flush();
                }
            }
        }
        Some("scroll") => {
            let dir = v.get("dir").and_then(|d| d.as_i64()).unwrap_or(0);
            let lines = v.get("lines").and_then(|l| l.as_u64()).unwrap_or(3) as usize;
            let col = v.get("col").and_then(|c| c.as_u64()).unwrap_or(1) as usize;
            let row = v.get("row").and_then(|r| r.as_u64()).unwrap_or(1) as usize;
            if dir != 0 && lines != 0 {
                scroll_session(s, dir, lines, col, row);
            }
        }
        Some("mouse") => {
            let kind = v.get("kind").and_then(|k| k.as_u64()).unwrap_or(0) as u8;
            let button = v.get("button").and_then(|b| b.as_i64()).unwrap_or(0);
            let col = v.get("col").and_then(|c| c.as_u64()).unwrap_or(1) as usize;
            let row = v.get("row").and_then(|r| r.as_u64()).unwrap_or(1) as usize;
            let mods = v.get("mods").and_then(|m| m.as_i64()).unwrap_or(0);
            mouse_session(s, kind, button, col, row, mods);
        }
        Some("resize") => {
            let cols = v.get("cols").and_then(|c| c.as_u64()).unwrap_or(0) as u16;
            let rows = v.get("rows").and_then(|r| r.as_u64()).unwrap_or(0) as u16;
            if cols >= 2 && rows >= 2 {
                resize_session(s, cols, rows);
            }
        }
        Some("focus") => {
            let focused = v.get("focused").and_then(|f| f.as_bool()).unwrap_or(true);
            focus_session(s, focused);
        }
        Some("kill") => kill_session(s),
        _ => {}
    }
}

async fn handle_ws(ws: WebSocketStream<TcpStream>, params: Params) {
    let (mut tx, mut rx) = ws.split();
    let (session, reused) = match get_or_create(&params).await {
        Ok(v) => v,
        Err(e) => {
            let msg = serde_json::json!({ "t": "error", "msg": e }).to_string();
            let _ = tx.send(Message::Text(msg)).await;
            return;
        }
    };
    resize_session(&session, params.cols, params.rows);

    let ready = serde_json::json!({
        "t": "ready",
        "cols": session.cols.load(Ordering::Relaxed),
        "rows": session.rows.load(Ordering::Relaxed),
        "reused": reused,
        "resumeId": read_binding(&params.tile_id),
    })
    .to_string();
    if tx.send(Message::Text(ready)).await.is_err() {
        return;
    }
    if tx
        .send(Message::Binary(build_frame(&session)))
        .await
        .is_err()
    {
        return;
    }
    session.dirty.store(false, Ordering::Relaxed);

    let mut tick = tokio::time::interval(Duration::from_millis(33));
    let mut claude = ClaudeTracker::default();
    let mut claude_tick = tokio::time::interval(Duration::from_millis(800));
    loop {
        tokio::select! {
            _ = claude_tick.tick() => {
                if let Some(msg) = claude.poll(&params.tile_id) {
                    if tx.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                if session.exited.load(Ordering::Relaxed) {
                    let _ = tx.send(Message::Text("{\"t\":\"exit\"}".into())).await;
                    break;
                }
                if session.dirty.swap(false, Ordering::Relaxed) {
                    if tx.send(Message::Binary(build_frame(&session))).await.is_err() {
                        break;
                    }
                }
                if session.cwd_dirty.swap(false, Ordering::Relaxed) {
                    let cwd = session.cwd.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    let branch = session.branch.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    if let Some(cwd) = cwd {
                        let msg = serde_json::json!({ "t": "cwd", "cwd": cwd, "branch": branch }).to_string();
                        if tx.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                }
                if session.clipboard_dirty.swap(false, Ordering::Relaxed) {
                    let text = session.clipboard.lock().unwrap_or_else(|e| e.into_inner()).take();
                    if let Some(text) = text {
                        let msg = serde_json::json!({ "t": "clipboard", "text": text }).to_string();
                        if tx.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                }
                if session.title_dirty.swap(false, Ordering::Relaxed) {
                    let title = session.title.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    if let Some(title) = title {
                        let msg = serde_json::json!({ "t": "title", "title": title }).to_string();
                        if tx.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
                }
                let queued: Vec<String> = {
                    let mut ev = session.events.lock().unwrap_or_else(|e| e.into_inner());
                    if ev.is_empty() { Vec::new() } else { std::mem::take(&mut *ev) }
                };
                let mut send_failed = false;
                for msg in queued {
                    if tx.send(Message::Text(msg)).await.is_err() {
                        send_failed = true;
                        break;
                    }
                }
                if send_failed {
                    break;
                }
            }
            msg = rx.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => handle_client_msg(&session, &t),
                    Some(Ok(Message::Binary(b))) => {
                        if let Ok(mut w) = session.writer.lock() {
                            let _ = w.write_all(&b);
                            let _ = w.flush();
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

async fn read_head(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte).await {
            Ok(0) => return None,
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    return Some(buf);
                }
                if buf.len() > 16384 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
}

fn parse_head(head: &[u8]) -> (String, HashMap<String, String>) {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or("");
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    (path, headers)
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex_val(b[i + 1]), hex_val(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), percent_decode(v));
        }
    }
    map
}

async fn handle_conn(mut stream: TcpStream) {
    let head = match read_head(&mut stream).await {
        Some(h) => h,
        None => return,
    };
    let (path, headers) = parse_head(&head);
    let (path_only, query) = match path.split_once('?') {
        Some((p, q)) => (p, q),
        None => (path.as_str(), ""),
    };
    let is_ws = headers
        .get("upgrade")
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false);

    if is_ws && path_only == "/pty" {
        let key = match headers.get("sec-websocket-key") {
            Some(k) => k.clone(),
            None => return,
        };
        let accept = derive_accept_key(key.as_bytes());
        let resp = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        if stream.write_all(resp.as_bytes()).await.is_err() {
            return;
        }
        let q = parse_query(query);
        let tile_id = match q.get("tileId") {
            Some(t) if !t.is_empty() => t.clone(),
            _ => return,
        };
        let params = Params {
            tile_id,
            cols: q.get("cols").and_then(|c| c.parse().ok()).unwrap_or(80),
            rows: q.get("rows").and_then(|r| r.parse().ok()).unwrap_or(24),
            cwd: q.get("cwd").cloned(),
            shell: q.get("shell").cloned(),
            target: q.get("target").cloned(),
        };
        let ws = WebSocketStream::from_raw_socket(stream, Role::Server, None).await;
        handle_ws(ws, params).await;
        return;
    }

    if path_only == "/agent-status" {
        let len = headers
            .get("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        if len > 0 && len <= STATUS_POST_MAX {
            let mut body = vec![0u8; len];
            if stream.read_exact(&mut body).await.is_ok() {
                handle_agent_status(&body);
            }
        }
        let _ = stream
            .write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n")
            .await;
        return;
    }

    if path_only == "/capture" {
        let q = parse_query(query);
        let tid = q.get("tileId").cloned().unwrap_or_default();
        let lines = q.get("lines").and_then(|l| l.parse().ok()).unwrap_or(1000usize);
        let (raw, cols) = match sessions().lock().unwrap().get(&tid).cloned() {
            Some(s) => (
                s.raw.lock().map(|r| r.clone()).unwrap_or_default(),
                s.cols.load(Ordering::Relaxed),
            ),
            None => (load_buffer(&tid), 80),
        };
        let text = capture_text(&raw, cols, lines);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            text.len(),
            text
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }

    if path_only == "/foreground" {
        let q = parse_query(query);
        let tid = q.get("tileId").cloned().unwrap_or_default();
        let s = sessions().lock().unwrap().get(&tid).cloned();
        let name = match s {
            Some(s) => {
                let pid = s.child.lock().ok().and_then(|c| c.process_id());
                let shell = s.shell.clone();
                tokio::task::spawn_blocking(move || {
                    pid.and_then(foreground_command)
                        .unwrap_or_else(|| display_command_name(&shell))
                })
                .await
                .unwrap_or_default()
            }
            None => String::new(),
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            name.len(),
            name
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }

    let body = if path_only == "/kill" {
        let q = parse_query(query);
        if let Some(tid) = q.get("tileId") {
            let s = sessions().lock().unwrap().get(tid).cloned();
            if let Some(s) = s {
                kill_session(&s);
            }
        }
        "ok"
    } else if path_only == "/health" {
        "ok"
    } else {
        "panorama sidecar"
    };
    let resp = format!(
        "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}

#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("record-agent") {
        record_agent();
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("emit-event") {
        let event = std::env::args().nth(2).unwrap_or_default();
        emit_event(&event);
        return;
    }
    if std::env::args().nth(1).as_deref() == Some("statusline") {
        statusline_cmd();
        return;
    }
    install_claude_hook();
    let listener = TcpListener::bind(("127.0.0.1", PORT))
        .await
        .expect("bind 9777");
    tokio::spawn(async {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tick.tick().await;
            let list: Vec<Arc<Session>> = sessions().lock().unwrap().values().cloned().collect();
            let _ = tokio::task::spawn_blocking(move || {
                for s in list {
                    flush_session(&s);
                }
            })
            .await;
        }
    });
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let _ = stream.set_nodelay(true);
                tokio::spawn(handle_conn(stream));
            }
            Err(_) => continue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_event_ws_msg, decode_osc52, default_status_line, sanitize_event_text,
        status_ws_msg, utf8_valid_len, Arc, AtomicBool, CwdSink, Ordering,
        OSC_BG_RESPONSE, OSC_FG_RESPONSE,
    };

    #[test]
    fn statusline_json_becomes_claude_msg() {
        let input = serde_json::json!({
            "session_id": "s1",
            "model": { "id": "claude-fable-5", "display_name": "Fable 5" },
            "effort": { "level": "high" },
            "thinking": { "enabled": true },
            "context_window": {
                "total_input_tokens": 15500,
                "context_window_size": 1000000,
                "used_percentage": 8.2
            },
            "cost": { "total_cost_usd": 0.5 },
            "rate_limits": { "five_hour": { "used_percentage": 23.5 } }
        });
        let msg = status_ws_msg(&input).unwrap();
        let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(v["t"], "claude");
        assert_eq!(v["model"], "claude-fable-5");
        assert_eq!(v["effort"], "high");
        assert_eq!(v["contextWindow"], 1000000);
        assert_eq!(v["contextPercent"], 8.2);
        assert_eq!(v["rateFiveHour"], 23.5);
        assert!(v.get("rateSevenDay").is_none());
        assert_eq!(default_status_line(&input), "Fable 5 \u{00b7} 8% ctx \u{b7} $0.50");
        assert!(status_ws_msg(&serde_json::json!({"session_id": "x"})).is_none());
    }

    #[test]
    fn duration_formatting() {
        assert_eq!(super::fmt_duration(9), "9s");
        assert_eq!(super::fmt_duration(154), "2m34s");
        assert_eq!(super::fmt_duration(3725), "1h02m");
    }

    #[test]
    fn git_branch_resolves_repo_head() {
        let branch = super::git_branch(env!("CARGO_MANIFEST_DIR"));
        assert!(branch.is_some());
        assert!(!branch.unwrap().is_empty());
        assert_eq!(super::git_branch("C:\\"), None);
    }

    #[test]
    fn osc9_progress_and_notifications() {
        let mut parser = vt100::Parser::new_with_callbacks(24, 80, 0, CwdSink::default());
        parser.process(b"\x1b]9;4;1;42\x07\x1b]9;2;task done\x07\x1b]9;plain notify\x07");
        assert_eq!(parser.callbacks_mut().take_progress(), Some((1, 42)));
        assert_eq!(parser.callbacks_mut().take_progress(), None);
        let notifies = parser.callbacks_mut().take_notifies();
        assert_eq!(
            notifies,
            vec![
                (String::new(), "task done".to_string()),
                (String::new(), "plain notify".to_string())
            ]
        );
        parser.process(b"\x1b]9;4;0;0\x07");
        assert_eq!(parser.callbacks_mut().take_progress(), Some((0, 0)));
    }

    #[test]
    fn osc777_routes_agent_events_and_generic_notifies() {
        let mut parser = vt100::Parser::new_with_callbacks(24, 80, 0, CwdSink::default());
        let body = r#"{"v":1,"event":"stop","response":"a;b;c"}"#;
        parser.process(format!("\x1b]777;notify;panorama://cli-agent;{body}\x07").as_bytes());
        parser.process(b"\x1b]777;notify;Build;done in 3s\x07");
        let events = parser.callbacks_mut().take_agent_events();
        assert_eq!(events, vec![body.to_string()]);
        let notifies = parser.callbacks_mut().take_notifies();
        assert_eq!(notifies, vec![("Build".to_string(), "done in 3s".to_string())]);
    }

    #[test]
    fn agent_event_body_becomes_ws_message() {
        let body = r#"{"v":1,"agent":"claude","event":"stop","session_id":"s1","project":"p","response":"done"}"#;
        let msg = agent_event_ws_msg(body).unwrap();
        let v: serde_json::Value = serde_json::from_str(&msg).unwrap();
        assert_eq!(v["t"], "agentEvent");
        assert_eq!(v["event"], "stop");
        assert_eq!(v["sessionId"], "s1");
        assert_eq!(v["response"], "done");
        assert!(agent_event_ws_msg("not json").is_none());
    }

    #[test]
    fn event_text_is_sanitized_and_capped() {
        assert_eq!(sanitize_event_text("a\x1b[31m b\n\tc"), "a [31m b c");
        let long = "x".repeat(500);
        let out = sanitize_event_text(&long);
        assert_eq!(out.chars().count(), 200);
        assert!(out.ends_with("..."));
    }

    #[test]
    fn color_queries_queue_responses_and_title_is_captured() {
        let mut parser = vt100::Parser::new_with_callbacks(24, 80, 0, CwdSink::default());
        parser.process(b"\x1b]10;?\x07\x1b]11;?\x07\x1b]2;my title\x07");
        let responses = parser.callbacks_mut().take_responses();
        assert_eq!(responses, vec![OSC_FG_RESPONSE.to_vec(), OSC_BG_RESPONSE.to_vec()]);
        assert_eq!(parser.callbacks_mut().take_title().as_deref(), Some("my title"));
        assert_eq!(parser.callbacks_mut().take_title(), None);
    }

    #[test]
    fn focus_reporting_mode_tracked_and_reports_current_state() {
        let focused = Arc::new(AtomicBool::new(true));
        let reporting = Arc::new(AtomicBool::new(false));
        let sink = CwdSink {
            focused: focused.clone(),
            focus_reporting: reporting.clone(),
            ..Default::default()
        };
        let mut parser = vt100::Parser::new_with_callbacks(24, 80, 0, sink);
        parser.process(b"\x1b[?1004h");
        assert!(reporting.load(Ordering::Relaxed));
        assert_eq!(parser.callbacks_mut().take_responses(), vec![b"\x1b[I".to_vec()]);
        parser.process(b"\x1b[?1004l");
        assert!(!reporting.load(Ordering::Relaxed));
        assert!(parser.callbacks_mut().take_responses().is_empty());
    }

    #[test]
    fn osc52_decodes_base64_payload() {
        assert_eq!(decode_osc52(b"aGVsbG8gd29ybGQ=").as_deref(), Some("hello world"));
        assert_eq!(decode_osc52(b"aGVs\nbG8=").as_deref(), Some("hello"));
        assert_eq!(decode_osc52(b"?"), None);
        assert_eq!(decode_osc52(b""), None);
        assert_eq!(decode_osc52(b"!!!not base64!!!"), None);
    }

    #[test]
    fn multibyte_split_across_reads_reassembles() {
        let text = "via \u{f0} v1 \u{2601}\u{fe0f}".as_bytes();
        let mut carry: Vec<u8> = Vec::new();
        let mut out: Vec<u8> = Vec::new();
        for byte in text {
            let mut data = std::mem::take(&mut carry);
            data.push(*byte);
            let valid = utf8_valid_len(&data);
            carry.extend_from_slice(&data[valid..]);
            out.extend_from_slice(&data[..valid]);
            assert!(std::str::from_utf8(&data[..valid]).is_ok());
        }
        assert!(carry.is_empty());
        assert_eq!(out, text);
    }

    #[test]
    fn genuine_invalid_byte_not_held() {
        assert_eq!(utf8_valid_len(&[0xff, 0xfe]), 2);
    }

    #[test]
    fn tracker_ingests_model_mode_tokens() {
        let mut t = super::ClaudeTracker::default();
        t.ingest(r#"{"type":"mode","mode":"plan"}"#);
        t.ingest(r#"{"type":"permission-mode","permissionMode":"acceptEdits"}"#);
        t.ingest(
            r#"{"message":{"role":"assistant","model":"claude-opus-4-8","usage":{"input_tokens":10,"cache_read_input_tokens":5,"cache_creation_input_tokens":2}}}"#,
        );
        assert_eq!(t.mode.as_deref(), Some("plan"));
        assert_eq!(t.perm.as_deref(), Some("acceptEdits"));
        assert_eq!(t.model.as_deref(), Some("claude-opus-4-8"));
        assert_eq!(t.tokens, Some(17));
    }

    #[test]
    fn status_matches_by_session_id() {
        let raw = r#"{"pid":105884,"sessionId":"abc-123","cwd":"C:\\Users\\x","status":"waiting"}"#;
        assert_eq!(super::status_from_session_json(raw, "abc-123").as_deref(), Some("waiting"));
        assert_eq!(super::status_from_session_json(raw, "other"), None);
    }

    #[test]
    fn tracker_ignores_non_assistant_usage() {
        let mut t = super::ClaudeTracker::default();
        t.ingest(r#"{"message":{"role":"user","usage":{"input_tokens":99}}}"#);
        assert_eq!(t.model, None);
        assert_eq!(t.tokens, None);
    }

    #[test]
    fn usage_tokens_zero_is_none() {
        assert_eq!(super::usage_tokens(&serde_json::json!({"input_tokens": 0})), None);
        assert_eq!(super::usage_tokens(&serde_json::json!({"input_tokens": 3})), Some(3));
    }

    #[test]
    fn cwd_slug_replaces_separators() {
        assert_eq!(super::cwd_slug("D:/workspace/panorama-term"), "D--workspace-panorama-term");
    }

    #[test]
    fn osc7_parses_windows_and_unix_paths() {
        use super::parse_osc7;
        assert_eq!(
            parse_osc7(b"file://HOST/D:/workspace/panorama-term").as_deref(),
            Some("D:/workspace/panorama-term")
        );
        assert_eq!(
            parse_osc7(b"file://host/home/u/my%20dir").as_deref(),
            Some("/home/u/my dir")
        );
        assert_eq!(parse_osc7(b"not-a-url"), None);
    }
}
