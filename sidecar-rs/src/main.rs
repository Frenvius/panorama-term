use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

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
}

impl vt100::Callbacks for CwdSink {
    fn unhandled_osc(&mut self, _screen: &mut vt100::Screen, params: &[&[u8]]) {
        if let [b"7", url] = params {
            if let Some(path) = parse_osc7(url) {
                if self.cwd.as_deref() != Some(path.as_str()) {
                    self.cwd = Some(path);
                    self.changed = true;
                }
            }
        }
    }
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

fn read_binding(tile_id: &str) -> Option<String> {
    let raw = std::fs::read_to_string(binding_path(tile_id)?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("agentSessionId")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
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
    let command = format!("\"{}\" record-agent", exe.display());
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

    let existing = hooks
        .get("SessionStart")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
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
                            .map(|c| c.contains("record-agent"))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        })
        .collect();
    cleaned.push(serde_json::json!({
        "hooks": [ { "type": "command", "command": command } ]
    }));
    hooks.insert("SessionStart".into(), serde_json::Value::Array(cleaned));

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
    let mut parser = vt100::Parser::new_with_callbacks(rows, cols, SCROLLBACK_LINES, CwdSink::default());
    if !seed.is_empty() {
        parser.process(&seed);
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
                    {
                        let mut p = s.parser.lock().unwrap_or_else(|e| e.into_inner());
                        p.process(chunk);
                        if let Some(new_cwd) = p.callbacks_mut().take_changed() {
                            *s.cwd.lock().unwrap_or_else(|e| e.into_inner()) = Some(new_cwd);
                            s.cwd_dirty.store(true, Ordering::Relaxed);
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
            let _ = w.write_all(&seq);
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
    drop(parser);

    let text_bytes = text.as_bytes();
    let mut buf = Vec::with_capacity(16 + text_bytes.len() + attrs.len() * 4);
    buf.push(1u8);
    buf.extend_from_slice(&rows.to_le_bytes());
    buf.extend_from_slice(&cols.to_le_bytes());
    buf.extend_from_slice(&cursor.to_le_bytes());
    buf.push(if hidden { 1 } else { 0 });
    buf.extend_from_slice(&offset.to_le_bytes());
    buf.extend_from_slice(&(text_bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(text_bytes);
    for w in &attrs {
        buf.extend_from_slice(&w.to_le_bytes());
    }
    buf
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
        Some("resize") => {
            let cols = v.get("cols").and_then(|c| c.as_u64()).unwrap_or(0) as u16;
            let rows = v.get("rows").and_then(|r| r.as_u64()).unwrap_or(0) as u16;
            if cols >= 2 && rows >= 2 {
                resize_session(s, cols, rows);
            }
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
    loop {
        tokio::select! {
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
                    if let Some(cwd) = cwd {
                        let msg = serde_json::json!({ "t": "cwd", "cwd": cwd }).to_string();
                        if tx.send(Message::Text(msg)).await.is_err() {
                            break;
                        }
                    }
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
    use super::utf8_valid_len;

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
