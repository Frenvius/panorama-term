use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel as mpsc_channel, Receiver as MpscReceiver, Sender as MpscSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::Role;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

mod host;

enum ConsumerMsg {
    Data(Vec<u8>),
    Exit { code: Option<u32> },
}

struct HostHandle {
    tx: MpscSender<Vec<u8>>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, MpscSender<serde_json::Value>>>,
    consumers: Mutex<HashMap<String, MpscSender<ConsumerMsg>>>,
    reader_stream: Mutex<Option<std::net::TcpStream>>,
}

impl HostHandle {
    fn rpc_raw(&self, payload: serde_json::Value) -> Result<serde_json::Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut msg = payload;
        msg["id"] = serde_json::json!(id);
        let (reply_tx, reply_rx) = mpsc_channel();
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).insert(id, reply_tx);
        let frame = host::encode(host::KIND_RPC, msg.to_string().as_bytes());
        self.tx.send(frame).map_err(|e| format!("host send: {e}"))?;
        reply_rx.recv().map_err(|e| format!("host reply: {e}"))
    }

    fn spawn_pty(
        &self,
        key: &str,
        cwd: &str,
        cols: u16,
        rows: u16,
        exe: &str,
        args: &[String],
        env: &[(String, String)],
        seed: &[u8],
        meta: serde_json::Value,
    ) -> Result<u32, String> {
        let seed_b64 = base64::engine::general_purpose::STANDARD.encode(seed);
        let reply = self.rpc_raw(serde_json::json!({
            "op": "spawn",
            "key": key,
            "cwd": cwd,
            "cols": cols,
            "rows": rows,
            "exe": exe,
            "args": args,
            "env": env.iter().map(|(k, v)| [k, v]).collect::<Vec<_>>(),
            "seed": seed_b64,
            "meta": meta,
        }))?;
        if !reply["ok"].as_bool().unwrap_or(false) {
            return Err(reply["err"].as_str().unwrap_or("spawn failed").to_string());
        }
        Ok(reply["pid"].as_u64().unwrap_or(0) as u32)
    }

    fn register_consumer(&self, key: &str, tx: MpscSender<ConsumerMsg>) {
        self.consumers.lock().unwrap_or_else(|e| e.into_inner()).insert(key.to_string(), tx);
    }

    fn subscribe(&self, key: &str, from_offset: u64) -> Result<(), String> {
        let reply = self.rpc_raw(serde_json::json!({
            "op": "subscribe",
            "key": key,
            "from_offset": from_offset,
        }))?;
        if !reply["ok"].as_bool().unwrap_or(false) {
            return Err(reply["err"].as_str().unwrap_or("subscribe failed").to_string());
        }
        Ok(())
    }

    fn write(&self, key: &str, bytes: &[u8]) {
        let payload = host::encode_input_payload(key, bytes);
        let _ = self.tx.send(host::encode(host::KIND_INPUT, &payload));
    }

    fn resize(&self, key: &str, cols: u16, rows: u16) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = host::encode(
            host::KIND_RPC,
            serde_json::json!({ "id": id, "op": "resize", "key": key, "cols": cols, "rows": rows })
                .to_string()
                .as_bytes(),
        );
        let _ = self.tx.send(frame);
    }

    fn kill(&self, key: &str) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let frame = host::encode(
            host::KIND_RPC,
            serde_json::json!({ "id": id, "op": "kill", "key": key })
                .to_string()
                .as_bytes(),
        );
        let _ = self.tx.send(frame);
    }

    fn session_info(&self, key: &str) -> Option<(bool, u16)> {
        let reply = self.rpc_raw(serde_json::json!({ "op": "list" })).ok()?;
        let found = reply["sessions"]
            .as_array()?
            .iter()
            .find(|s| s["key"].as_str() == Some(key))?;
        Some((
            found["alive"].as_bool().unwrap_or(false),
            found["cols"].as_u64().unwrap_or(80) as u16,
        ))
    }

    fn snapshot(&self, key: &str) -> Vec<u8> {
        let reply = match self.rpc_raw(serde_json::json!({"op": "snapshot", "key": key})) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        if !reply["ok"].as_bool().unwrap_or(false) {
            return Vec::new();
        }
        base64::engine::general_purpose::STANDARD
            .decode(reply["data"].as_str().unwrap_or(""))
            .unwrap_or_default()
    }
}

fn brain_dispatcher(h: &'static HostHandle, mut stream: std::net::TcpStream) {
    use std::io::Read;
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                while let Some((kind, payload, consumed)) = host::decode(&pending) {
                    pending.drain(0..consumed);
                    match kind {
                        host::KIND_DATA => {
                            if let Some((key, _offset, bytes)) =
                                host::decode_data_payload(&payload)
                            {
                                let consumers =
                                    h.consumers.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(tx) = consumers.get(&key) {
                                    let _ = tx.send(ConsumerMsg::Data(bytes));
                                }
                            }
                        }
                        host::KIND_RPC_REPLY => {
                            if let Ok(v) =
                                serde_json::from_slice::<serde_json::Value>(&payload)
                            {
                                let id = v["id"].as_u64().unwrap_or(u64::MAX);
                                let mut p =
                                    h.pending.lock().unwrap_or_else(|e| e.into_inner());
                                if let Some(tx) = p.remove(&id) {
                                    let _ = tx.send(v);
                                }
                            }
                        }
                        host::KIND_EVENT => {
                            if let Ok(ev) =
                                serde_json::from_slice::<serde_json::Value>(&payload)
                            {
                                if ev["kind"] == "exit" {
                                    let key =
                                        ev["key"].as_str().unwrap_or("").to_string();
                                    let code = ev["code"].as_u64().map(|c| c as u32);
                                    let consumers =
                                        h.consumers.lock().unwrap_or_else(|e| e.into_inner());
                                    if let Some(tx) = consumers.get(&key) {
                                        let _ = tx.send(ConsumerMsg::Exit { code });
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn host_port() -> u16 {
    std::env::var("PANORAMA_HOST_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(9778)
}

fn connect_to_host() -> std::net::TcpStream {
    let port = host_port();
    loop {
        match std::net::TcpStream::connect(("127.0.0.1", port)) {
            Ok(s) => return s,
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

fn host_handle() -> &'static HostHandle {
    static H: OnceLock<HostHandle> = OnceLock::new();
    H.get_or_init(|| {
        let stream = connect_to_host();
        let write_stream = stream.try_clone().expect("tcp clone");
        let (writer_tx, writer_rx) = mpsc_channel::<Vec<u8>>();
        std::thread::spawn(move || {
            use std::io::Write;
            let mut ws = write_stream;
            while let Ok(frame) = writer_rx.recv() {
                if ws.write_all(&frame).is_err() {
                    break;
                }
            }
        });
        HostHandle {
            tx: writer_tx,
            next_id: AtomicU64::new(1),
            pending: Mutex::new(HashMap::new()),
            consumers: Mutex::new(HashMap::new()),
            reader_stream: Mutex::new(Some(stream)),
        }
    });
    static DISP: OnceLock<()> = OnceLock::new();
    DISP.get_or_init(|| {
        let handle: &'static HostHandle = H.get().unwrap();
        let stream = handle
            .reader_stream
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .unwrap();
        std::thread::spawn(move || brain_dispatcher(handle, stream));
    });
    H.get().unwrap()
}

fn reconnect_sessions() {
    let reply = match host_handle().rpc_raw(serde_json::json!({"op": "list"})) {
        Ok(r) => r,
        Err(_) => return,
    };
    let empty = vec![];
    let session_list = reply["sessions"].as_array().unwrap_or(&empty).clone();
    for si in &session_list {
        let key = si["key"].as_str().unwrap_or("").to_string();
        if key.is_empty() {
            continue;
        }
        let alive = si["alive"].as_bool().unwrap_or(false);
        let pid = si["pid"].as_u64().unwrap_or(0) as u32;
        let cols = (si["cols"].as_u64().unwrap_or(220) as u16).max(2);
        let rows = (si["rows"].as_u64().unwrap_or(50) as u16).max(2);
        let snap_total = si["total"].as_u64().unwrap_or(0);

        let raw = host_handle().snapshot(&key);

        let focused = Arc::new(AtomicBool::new(false));
        let focus_reporting = Arc::new(AtomicBool::new(false));
        let sink = CwdSink {
            focused: focused.clone(),
            focus_reporting: focus_reporting.clone(),
            ..Default::default()
        };
        let mut parser =
            vt100::Parser::new_with_callbacks(rows, cols, SCROLLBACK_LINES, sink);
        if !raw.is_empty() {
            parser.process(&raw);
            let _ = parser.callbacks_mut().take_clipboard();
            let _ = parser.callbacks_mut().take_responses();
            let _ = parser.callbacks_mut().take_agent_events();
            let _ = parser.callbacks_mut().take_notifies();
            let _ = parser.callbacks_mut().take_progress();
        }

        let run = if key.starts_with("run:") || key.starts_with("build:") {
            let meta = &si["meta"];
            let run_kind = meta["kind"]
                .as_str()
                .unwrap_or(if key.starts_with("build:") { "build" } else { "run" });
            let mut log = RunLog::new();
            if !raw.is_empty() {
                if let Ok(text) = std::str::from_utf8(&raw) {
                    log.feed(text);
                }
            }
            Some(RunState {
                cmd: meta["cmd"].as_str().unwrap_or(&key).to_string(),
                cwd: meta["cwd"].as_str().unwrap_or("").to_string(),
                kind: run_kind.to_string(),
                started: Instant::now(),
                log: Mutex::new(log),
                exit: Mutex::new(None),
            })
        } else {
            None
        };

        let session = Arc::new(Session {
            tile_id: key.clone(),
            shell: key.clone(),
            pid,
            parser: Mutex::new(parser),
            dirty: AtomicBool::new(true),
            exited: AtomicBool::new(!alive),
            visible: AtomicBool::new(true),
            cols: AtomicU16::new(cols),
            rows: AtomicU16::new(rows),
            scrollback: AtomicUsize::new(0),
            cwd: Mutex::new(None),
            cwd_dirty: AtomicBool::new(false),
            branch: Mutex::new(None),
            cmd: Mutex::new(None),
            clipboard: Mutex::new(None),
            clipboard_dirty: AtomicBool::new(false),
            title: Mutex::new(None),
            title_dirty: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
            focused,
            focus_reporting,
            run,
        });

        if alive {
            let (consumer_tx, consumer_rx) = mpsc_channel::<ConsumerMsg>();
            host_handle().register_consumer(&key, consumer_tx);
            let _ = host_handle().subscribe(&key, snap_total);
            let s = session.clone();
            std::thread::spawn(move || run_consumer_loop(s, consumer_rx, None));
        }

        sessions().lock().unwrap().insert(key, session);
    }
}

const PORT: u16 = 9777;
const DAEMON_ARG: &str = "--daemon";

fn port() -> u16 {
    std::env::var("PANORAMA_SIDECAR_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(PORT)
}

#[cfg(windows)]
fn daemonize() {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let base = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    for flags in [base | CREATE_BREAKAWAY_FROM_JOB, base] {
        let spawned = Command::new(&exe)
            .arg(DAEMON_ARG)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(flags)
            .spawn()
            .is_ok();
        if spawned {
            std::process::exit(0);
        }
    }
}
fn brain_bin() -> PathBuf {
    std::env::var("PANORAMA_BRAIN_BIN")
        .ok()
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .unwrap_or_else(|| PathBuf::from("sidecar"))
}

fn brain_run_copy(bin: &Path) -> PathBuf {
    if !cfg!(debug_assertions) {
        return bin.to_path_buf();
    }
    let dst = std::env::temp_dir().join("panorama-brain.exe");
    if std::fs::copy(bin, &dst).is_ok() {
        dst
    } else {
        bin.to_path_buf()
    }
}

fn brain_alive() -> bool {
    let addr = format!("127.0.0.1:{}", port());
    addr.parse()
        .ok()
        .map(|a| std::net::TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
        .unwrap_or(false)
}

fn supervise_brain() {
    use std::process::Stdio;
    let bin = brain_bin();
    let host_p = host_port().to_string();
    let brain_p = port().to_string();
    std::thread::spawn(move || loop {
        if brain_alive() {
            std::thread::sleep(Duration::from_millis(1000));
            continue;
        }
        let exe = brain_run_copy(&bin);
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg(DAEMON_ARG)
            .env("PANORAMA_HOST_PORT", &host_p)
            .env("PANORAMA_SIDECAR_PORT", &brain_p)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        match cmd.spawn() {
            Ok(mut child) => {
                let _ = child.wait();
            }
            Err(e) => eprintln!("[host] failed to spawn brain: {e}"),
        }
        std::thread::sleep(Duration::from_millis(1000));
    });
}

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
const RUN_LINE_CAP: usize = 10_000;
const RUN_REQ_MAX: usize = 500;

#[derive(Clone, Copy, PartialEq)]
enum EscState {
    Ground,
    Esc,
    EscInter,
    Csi,
    Osc,
    OscEsc,
}

struct RunLog {
    buf: std::collections::VecDeque<String>,
    total: usize,
    partial: String,
    esc: EscState,
    cr: bool,
}

impl RunLog {
    fn new() -> Self {
        RunLog {
            buf: std::collections::VecDeque::new(),
            total: 0,
            partial: String::new(),
            esc: EscState::Ground,
            cr: false,
        }
    }

    fn push_line(&mut self) {
        self.total += 1;
        self.buf.push_back(std::mem::take(&mut self.partial));
        if self.buf.len() > RUN_LINE_CAP {
            self.buf.pop_front();
        }
    }

    fn feed(&mut self, text: &str) {
        for c in text.chars() {
            match self.esc {
                EscState::Ground => {
                    if self.cr && c != '\n' {
                        self.cr = false;
                        self.partial.clear();
                    }
                    match c {
                        '\x1b' => self.esc = EscState::Esc,
                        '\n' => {
                            self.cr = false;
                            self.push_line();
                        }
                        '\r' => self.cr = true,
                        '\t' => self.partial.push('\t'),
                        c if (c as u32) >= 0x20 => self.partial.push(c),
                        _ => {}
                    }
                }
                EscState::Esc => {
                    self.esc = match c {
                        '[' => EscState::Csi,
                        ']' => EscState::Osc,
                        '(' | ')' | '#' | '%' => EscState::EscInter,
                        _ => EscState::Ground,
                    }
                }
                EscState::EscInter => self.esc = EscState::Ground,
                EscState::Csi => {
                    if ('\u{40}'..='\u{7e}').contains(&c) {
                        self.esc = EscState::Ground;
                    }
                }
                EscState::Osc => match c {
                    '\x07' => self.esc = EscState::Ground,
                    '\x1b' => self.esc = EscState::OscEsc,
                    _ => {}
                },
                EscState::OscEsc => {
                    self.esc = if c == '\\' { EscState::Ground } else { EscState::Osc };
                }
            }
        }
    }

    fn range(&self) -> (usize, usize) {
        let first = self.total.saturating_sub(self.buf.len()) + 1;
        (first, self.total)
    }
}

struct RunState {
    cmd: String,
    cwd: String,
    kind: String,
    started: Instant,
    log: Mutex<RunLog>,
    exit: Mutex<Option<u32>>,
}

fn expand_win_env(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        match after.find('%') {
            Some(j) => {
                let name = &after[..j];
                match std::env::var(name) {
                    Ok(v) => out.push_str(&v),
                    Err(_) => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[j + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(windows)]
fn reg_path_value(key: &str) -> Option<String> {
    let out = hidden_command("reg.exe")
        .args(["query", key, "/v", "Path"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let idx = line
            .find("REG_EXPAND_SZ")
            .map(|i| i + "REG_EXPAND_SZ".len())
            .or_else(|| line.find("REG_SZ").map(|i| i + "REG_SZ".len()));
        if let Some(idx) = idx {
            let v = line[idx..].trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(windows)]
fn fresh_path() -> Option<String> {
    let machine = reg_path_value(r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment");
    let user = reg_path_value(r"HKCU\Environment");
    let current = std::env::var("PATH").ok();
    let mut parts: Vec<String> = Vec::new();
    for chunk in [machine, user, current].into_iter().flatten() {
        for seg in expand_win_env(&chunk).split(';') {
            let seg = seg.trim();
            if !seg.is_empty() && !parts.iter().any(|e| e.eq_ignore_ascii_case(seg)) {
                parts.push(seg.to_string());
            }
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(";"))
    }
}

#[cfg(not(windows))]
fn fresh_path() -> Option<String> {
    None
}

fn norm_cwd(cwd: &str) -> String {
    let mut norm = cwd.replace('/', if cfg!(windows) { "\\" } else { "/" });
    while norm.len() > 1 && (norm.ends_with('\\') || norm.ends_with('/')) {
        norm.pop();
    }
    if cfg!(windows) {
        norm = norm.to_lowercase();
    }
    norm
}

fn run_key(kind: &str, tile: &str) -> String {
    format!("{}:{}", kind, sanitize_key(tile))
}

fn run_lookup_tile(kind: &str, tile: &str) -> Option<Arc<Session>> {
    sessions().lock().unwrap().get(&run_key(kind, tile)).cloned()
}

fn run_lookup_cwd(cwd: &str) -> Option<Arc<Session>> {
    let norm = norm_cwd(cwd);
    let list: Vec<Arc<Session>> = sessions().lock().unwrap().values().cloned().collect();
    let mut best: Option<Arc<Session>> = None;
    for s in list {
        let Some(run) = &s.run else { continue };
        if run.kind != "run" || run.cwd != norm {
            continue;
        }
        let better = match &best {
            None => true,
            Some(b) => {
                let b_run = b.run.as_ref().unwrap();
                let s_live = !s.exited.load(Ordering::Relaxed);
                let b_live = !b.exited.load(Ordering::Relaxed);
                (s_live && !b_live) || (s_live == b_live && run.started > b_run.started)
            }
        };
        if better {
            best = Some(s);
        }
    }
    best
}

fn run_lookup(kind: &str, tile: Option<&str>, cwd: Option<&str>) -> Option<Arc<Session>> {
    match tile {
        Some(t) if !t.is_empty() => run_lookup_tile(kind, t),
        _ => cwd.filter(|c| !c.is_empty()).and_then(run_lookup_cwd),
    }
}

pub(crate) struct RawRing {
    pub(crate) buf: Vec<u8>,
    pub(crate) base: u64,
}

impl RawRing {
    pub(crate) fn new(seed: Vec<u8>) -> Self {
        Self { base: 0, buf: seed }
    }

    pub(crate) fn push(&mut self, data: &[u8]) {
        self.buf.extend_from_slice(data);
        if self.buf.len() > RAW_CAP {
            let cut = self.buf.len() - RAW_CAP;
            self.buf.drain(0..cut);
            self.base += cut as u64;
        }
    }

    pub(crate) fn len_total(&self) -> u64 {
        self.base + self.buf.len() as u64
    }

    pub(crate) fn since(&self, offset: u64) -> (u64, Vec<u8>) {
        let total = self.len_total();
        if offset >= total {
            return (total, Vec::new());
        }
        let start = offset.max(self.base);
        let skip = (start - self.base) as usize;
        (start, self.buf[skip..].to_vec())
    }

    pub(crate) fn snapshot(&self) -> Vec<u8> {
        self.buf.clone()
    }
}

struct Session {
    tile_id: String,
    shell: String,
    pid: u32,
    parser: Mutex<vt100::Parser<CwdSink>>,
    dirty: AtomicBool,
    exited: AtomicBool,
    visible: AtomicBool,
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
    run: Option<RunState>,
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
    if s.run.is_some() {
        return;
    }
    let data = host_handle().snapshot(&s.tile_id);
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

fn clear_binding_session(tile_id: &str) {
    let Some(path) = binding_path(tile_id) else {
        return;
    };
    let Some(mut rec) = read_binding_rec(tile_id).filter(|v| v.is_object()) else {
        return;
    };
    let obj = rec.as_object_mut().unwrap();
    if obj.remove("agentSessionId").is_some() {
        let _ = std::fs::write(path, rec.to_string());
    }
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
    added: u64,
    removed: u64,
    effort: Option<String>,
    default_model: Option<String>,
    status: Option<String>,
    status_path: Option<PathBuf>,
    sent: Option<String>,
    reset: bool,
}

fn line_count(s: &str) -> u64 {
    if s.is_empty() { 0 } else { s.lines().count() as u64 }
}

fn diff_lines(old: &str, new: &str) -> (u64, u64) {
    let o: Vec<&str> = old.lines().collect();
    let n: Vec<&str> = new.lines().collect();
    let mut lead = 0;
    while lead < o.len() && lead < n.len() && o[lead] == n[lead] {
        lead += 1;
    }
    let mut trail = 0;
    while trail < o.len() - lead && trail < n.len() - lead
        && o[o.len() - 1 - trail] == n[n.len() - 1 - trail]
    {
        trail += 1;
    }
    ((n.len() - lead - trail) as u64, (o.len() - lead - trail) as u64)
}

impl ClaudeTracker {
    fn count_edits(&mut self, content: &serde_json::Value) {
        let Some(blocks) = content.as_array() else { return };
        let field = |v: &serde_json::Value, k: &str| {
            v.get(k).and_then(|s| s.as_str()).unwrap_or("").to_string()
        };
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                continue;
            }
            let Some(input) = block.get("input") else { continue };
            match block.get("name").and_then(|n| n.as_str()).unwrap_or("") {
                "Edit" => {
                    let (a, r) = diff_lines(&field(input, "old_string"), &field(input, "new_string"));
                    self.added += a;
                    self.removed += r;
                }
                "Write" => self.added += line_count(&field(input, "content")),
                "NotebookEdit" => {
                    let (a, r) = diff_lines(&field(input, "old_source"), &field(input, "new_source"));
                    self.added += a;
                    self.removed += r;
                }
                "MultiEdit" => {
                    if let Some(edits) = input.get("edits").and_then(|e| e.as_array()) {
                        for e in edits {
                            let (a, r) = diff_lines(&field(e, "old_string"), &field(e, "new_string"));
                            self.added += a;
                            self.removed += r;
                        }
                    }
                }
                _ => {}
            }
        }
    }

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
        if entry
            .pointer("/attachment/hookName")
            .and_then(|h| h.as_str())
            .is_some_and(|n| n == "SessionStart:clear")
        {
            self.added = 0;
            self.removed = 0;
            self.tokens = None;
            self.reset = true;
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
        if let Some(content) = msg.and_then(|m| m.get("content")) {
            self.count_edits(content);
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
        if self.added > 0 || self.removed > 0 {
            obj.insert("linesAdded".into(), self.added.into());
            obj.insert("linesRemoved".into(), self.removed.into());
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
    let addr: std::net::SocketAddr = format!("127.0.0.1:{}", port()).parse().unwrap();
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
    let mut out = serde_json::json!({ "terminalSequence": seq });
    if event == "prompt-submit" {
        let parts: Vec<String> = [linked_notes_context(), linked_peers_context(), run_context(&cwd)]
            .into_iter()
            .flatten()
            .collect();
        if !parts.is_empty() {
            out.as_object_mut().unwrap().insert(
                "hookSpecificOutput".into(),
                serde_json::json!({
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": parts.join("\n\n"),
                }),
            );
        }
    }
    println!("{out}");
}

fn urlenc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn sidecar_get(path_query: &str) -> Option<String> {
    use std::net::TcpStream as StdTcp;
    let addr = format!("127.0.0.1:{}", port());
    let stream = StdTcp::connect_timeout(&addr.parse().ok()?, Duration::from_millis(300)).ok()?;
    stream.set_read_timeout(Some(Duration::from_millis(500))).ok()?;
    stream.set_write_timeout(Some(Duration::from_millis(500))).ok()?;
    let mut stream = stream;
    let req = format!("GET {path_query} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).ok()?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).ok()?;
    raw.split_once("\r\n\r\n").map(|(_, b)| b.to_string())
}

fn run_context(cwd: &str) -> Option<String> {
    if cwd.is_empty() {
        return None;
    }
    let body = sidecar_get(&format!("/run/status?cwd={}", urlenc(cwd)))?;
    let v: serde_json::Value = serde_json::from_str(&body).ok()?;
    let state = v.get("state").and_then(|s| s.as_str())?;
    if state == "none" {
        return None;
    }
    let cmd = v.get("cmd").and_then(|c| c.as_str()).unwrap_or("?");
    let base = format!("http://127.0.0.1:{}", port());
    let enc = urlenc(cwd);
    Some(format!(
        "This project has a dev run session (`{cmd}`, currently {state}). When the user asks about the running app, its output, or errors in it, inspect the logs yourself:\n\
         - status: curl \"{base}/run/status?cwd={enc}\"\n\
         - logs: curl \"{base}/run/logs?cwd={enc}&tail=50\"\n\
         Logs are numbered lines; the first response line reports the served range and total (max 500 per request). Start with a small tail (30-50 lines) and paginate with &from=<line>&count=<n> only when needed. Remember the last line you read and fetch only newer lines on later checks. Never fetch the whole log."
    ))
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
            let mut rec = read_binding_rec(&tile_id)
                .filter(|v| v.is_object())
                .unwrap_or_else(|| serde_json::json!({}));
            let obj = rec.as_object_mut().unwrap();
            obj.insert("agentSessionId".into(), session_id.into());
            obj.insert("cwd".into(), cwd.map(Into::into).unwrap_or(serde_json::Value::Null));
            obj.insert("tileId".into(), tile_id.clone().into());
            let _ = std::fs::write(path, rec.to_string());
        }
    }
}

fn front_title(path: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let rest = raw
        .strip_prefix("---\n")
        .or_else(|| raw.strip_prefix("---\r\n"))?;
    let end = rest.find("\n---")?;
    for line in rest[..end].lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("title:") {
            let v = v.trim();
            if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                return Some(v[1..v.len() - 1].replace("\\\"", "\""));
            }
            if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') {
                return Some(v[1..v.len() - 1].replace("''", "'"));
            }
            return Some(v.to_string());
        }
    }
    None
}

fn linked_notes_context() -> Option<String> {
    let tile_id = std::env::var("PANORAMA_TILE_ID").ok()?;
    let rec = read_binding_rec(&tile_id)?;
    let notes = rec.get("notes").and_then(|n| n.as_array())?;
    let mut lines: Vec<String> = notes
        .iter()
        .filter_map(|n| {
            let path = n.get("path").and_then(|p| p.as_str())?;
            if path.is_empty() {
                return None;
            }
            let title = front_title(path)
                .or_else(|| n.get("title").and_then(|t| t.as_str()).map(String::from))
                .unwrap_or_else(|| "note".to_string());
            Some(format!("- \"{title}\": {path}"))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    lines.insert(
        0,
        "Notes linked to this terminal are plain markdown files (checkbox = \"- [ ]\" unchecked / \"- [x]\" checked). The note title is the `title:` field in the leading YAML frontmatter; edit it there to rename. Read or edit these files only when the user asks:"
            .to_string(),
    );
    Some(lines.join("\n"))
}

fn linked_peers_context() -> Option<String> {
    let tile_id = std::env::var("PANORAMA_TILE_ID").ok()?;
    let rec = read_binding_rec(&tile_id)?;
    let peers = rec.get("peers").and_then(|p| p.as_array())?;
    let entries: Vec<String> = peers
        .iter()
        .filter_map(|p| {
            let id = p.get("tileId").and_then(|v| v.as_str())?;
            let name = p.get("name").and_then(|v| v.as_str()).unwrap_or("agent");
            Some(format!("- \"{name}\": tileId {id}"))
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    let base = format!("http://127.0.0.1:{}", port());
    Some(format!(
        "Other terminals on this canvas are linked to yours; each may run its own agent or shell. Only interact with them when the user asks (e.g. \"ask X to...\", \"tell X...\", \"check on X\"):\n\
         - send a message: curl -s -X POST \"{base}/agent/send?tileId=<tileId>\" --data-raw \"your message\"\n\
         - read its screen: curl -s \"{base}/capture?tileId=<tileId>&lines=80\"\n\
         A send types the message into that terminal and presses Enter. After sending, wait, then read the screen to see the reply; long tasks can take minutes, so poll with pauses instead of resending. Linked terminals:\n{}",
        entries.join("\n")
    ))
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
    elevated: bool,
    spawn_cmd: Option<String>,
    run_kind: String,
    attach_only: bool,
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

fn powershell_history_filter_snippet() -> String {
    [
        "",
        "if ($env:PANORAMA_TERMINAL -and (Get-Module -ListAvailable PSReadLine)) {",
        "    Set-PSReadLineOption -AddToHistoryHandler {",
        "        param($__panoLine)",
        r"        return ($__panoLine -notmatch '^\s*claude --resume ')",
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
        let blocks = [
            ("__panoOldPrompt", powershell_osc7_snippet()),
            ("__panoLine", powershell_history_filter_snippet()),
        ];
        let missing: Vec<&str> = blocks
            .iter()
            .filter(|(marker, _)| !existing.contains(marker))
            .map(|(_, snippet)| snippet.as_str())
            .collect();
        if missing.is_empty() {
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
            for snippet in missing {
                let _ = f.write_all(snippet.as_bytes());
            }
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
            let mut bytes = hook.into_bytes();
            bytes.push(b'\n');
            host_handle().write(&s.tile_id, &bytes);
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

    let (exe, args, mut env): (String, Vec<String>, Vec<(String, String)>) =
        if let Some(run_cmd) = &p.spawn_cmd {
            if cfg!(windows) {
                (
                    resolve_powershell(),
                    vec!["-NoLogo".into(), "-Command".into(), run_cmd.clone()],
                    Vec::new(),
                )
            } else {
                ("/bin/sh".into(), vec!["-lc".into(), run_cmd.clone()], Vec::new())
            }
        } else if p.elevated {
            if !command_exists("gsudo.exe") {
                return Err(
                    "gsudo not found - install it with: winget install gerardog.gsudo".into(),
                );
            }
            ("gsudo.exe".into(), vec![shell.clone()], Vec::new())
        } else {
            (shell.clone(), Vec::new(), Vec::new())
        };

    env.push(("TERM".into(), "xterm-256color".into()));
    env.push(("PANORAMA_TERMINAL".into(), "1".into()));
    env.push(("PANORAMA_TILE_ID".into(), p.tile_id.clone()));
    if p.spawn_cmd.is_some() {
        if let Some(path) = fresh_path() {
            env.push(("PATH".into(), path));
        }
    }

    let seed = if p.spawn_cmd.is_some() { Vec::new() } else { load_buffer(&p.tile_id) };

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

    let meta = match &p.spawn_cmd {
        Some(run_cmd) => serde_json::json!({
            "cmd": run_cmd,
            "cwd": norm_cwd(&cwd),
            "kind": p.run_kind,
        }),
        None => serde_json::Value::Null,
    };
    let pid =
        host_handle().spawn_pty(&p.tile_id, &cwd, cols, rows, &exe, &args, &env, &seed, meta)?;

    let (consumer_tx, consumer_rx) = mpsc_channel::<ConsumerMsg>();
    host_handle().register_consumer(&p.tile_id, consumer_tx);
    host_handle().subscribe(&p.tile_id, seed.len() as u64)?;

    let run = p.spawn_cmd.as_ref().map(|run_cmd| RunState {
        cmd: run_cmd.clone(),
        cwd: norm_cwd(&cwd),
        kind: p.run_kind.clone(),
        started: Instant::now(),
        log: Mutex::new(RunLog::new()),
        exit: Mutex::new(None),
    });
    let session = Arc::new(Session {
        tile_id: p.tile_id.clone(),
        shell: p.spawn_cmd.clone().unwrap_or_else(|| shell.clone()),
        pid,
        parser: Mutex::new(parser),
        dirty: AtomicBool::new(true),
        exited: AtomicBool::new(false),
        visible: AtomicBool::new(true),
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
        run,
    });

    let s = session.clone();
    std::thread::spawn(move || run_consumer_loop(s, consumer_rx, Some(permit)));

    if session.run.is_none() {
        inject_osc7(&session, &shell);
    }
    Ok(session)
}

fn run_consumer_loop(
    s: Arc<Session>,
    consumer_rx: MpscReceiver<ConsumerMsg>,
    permit: Option<tokio::sync::OwnedSemaphorePermit>,
) {
    let mut permit = permit;
    let mut carry: Vec<u8> = Vec::new();
    loop {
        match consumer_rx.recv() {
            Ok(ConsumerMsg::Data(raw)) => {
                permit.take();
                let mut data = Vec::with_capacity(carry.len() + raw.len());
                data.append(&mut carry);
                data.extend_from_slice(&raw);
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
                        let msg = serde_json::json!({
                            "t": "notify",
                            "title": title,
                            "body": body,
                        });
                        outgoing.push(msg.to_string());
                    }
                    if let Some((state, pct)) = p.callbacks_mut().take_progress() {
                        let msg = serde_json::json!({
                            "t": "progress",
                            "state": state,
                            "pct": pct,
                        });
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
                for resp in &responses {
                    host_handle().write(&s.tile_id, resp);
                }
                if let Some(run) = &s.run {
                    if let Ok(text) = std::str::from_utf8(chunk) {
                        run.log.lock().unwrap_or_else(|e| e.into_inner()).feed(text);
                    }
                }
                s.dirty.store(true, Ordering::Relaxed);
            }
            Ok(ConsumerMsg::Exit { code }) => {
                s.exited.store(true, Ordering::Relaxed);
                s.dirty.store(true, Ordering::Relaxed);
                if let Some(run) = &s.run {
                    let mut exit = run.exit.lock().unwrap_or_else(|e| e.into_inner());
                    if exit.is_none() {
                        *exit = code;
                    }
                }
                flush_session(&s);
                if s.run.is_none() {
                    sessions().lock().unwrap().remove(&s.tile_id);
                }
                return;
            }
            Err(_) => {
                s.exited.store(true, Ordering::Relaxed);
                s.dirty.store(true, Ordering::Relaxed);
                flush_session(&s);
                if s.run.is_none() {
                    sessions().lock().unwrap().remove(&s.tile_id);
                }
                return;
            }
        }
    }
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
    if p.attach_only {
        if let Some(s) = sessions().lock().unwrap().get(&p.tile_id).cloned() {
            return Ok((s, true));
        }
        return Err("session not running".into());
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
    host_handle().resize(&s.tile_id, cols, rows);
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
        for _ in 0..lines.max(1) {
            host_handle().write(&s.tile_id, &seq);
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
        for _ in 0..lines {
            host_handle().write(&s.tile_id, arrow);
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
    host_handle().write(&s.tile_id, &seq);
}

fn focus_session(s: &Session, focused: bool) {
    let prev = s.focused.swap(focused, Ordering::Relaxed);
    if prev == focused || !s.focus_reporting.load(Ordering::Relaxed) {
        return;
    }
    host_handle().write(&s.tile_id, if focused { b"\x1b[I" } else { b"\x1b[O" });
}

fn kill_session(s: &Arc<Session>) {
    let s = s.clone();
    std::thread::spawn(move || {
        let pid = s.pid;
        #[cfg(windows)]
        {
            let _ = hidden_command("taskkill.exe")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .status();
        }
        #[cfg(unix)]
        {
            unsafe {
                libc::killpg(pid as i32, libc::SIGTERM);
            }
            std::thread::sleep(Duration::from_millis(150));
            unsafe {
                libc::killpg(pid as i32, libc::SIGKILL);
            }
        }
        host_handle().kill(&s.tile_id);
        s.exited.store(true, Ordering::Relaxed);
        s.dirty.store(true, Ordering::Relaxed);
    });
}

fn run_stop(s: &Arc<Session>) {
    host_handle().write(&s.tile_id, b"\x03");
    let s = s.clone();
    std::thread::spawn(move || {
        for _ in 0..30 {
            if s.exited.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        kill_session(&s);
    });
}

fn run_status_json(target: Option<Arc<Session>>) -> String {
    let Some(s) = target else {
        return serde_json::json!({ "state": "none" }).to_string();
    };
    let Some(run) = &s.run else {
        return serde_json::json!({ "state": "none" }).to_string();
    };
    let total = run.log.lock().unwrap_or_else(|e| e.into_inner()).total;
    if s.exited.load(Ordering::Relaxed) {
        let code = *run.exit.lock().unwrap_or_else(|e| e.into_inner());
        serde_json::json!({
            "state": "exited",
            "exitCode": code,
            "cmd": run.cmd,
            "totalLines": total,
            "sessionId": s.tile_id,
        })
        .to_string()
    } else {
        serde_json::json!({
            "state": "running",
            "cmd": run.cmd,
            "pid": s.pid,
            "totalLines": total,
            "sessionId": s.tile_id,
        })
        .to_string()
    }
}

fn run_logs_text(target: Option<Arc<Session>>, q: &HashMap<String, String>) -> String {
    let Some(s) = target else {
        return "# no run session".into();
    };
    let Some(run) = &s.run else {
        return "# no run session".into();
    };
    let state = if s.exited.load(Ordering::Relaxed) { "exited" } else { "running" };
    let log = run.log.lock().unwrap_or_else(|e| e.into_inner());
    let (first, total) = log.range();
    if total == 0 {
        return format!("# lines 0-0 of 0 ({state})");
    }
    let count = q
        .get("count")
        .or_else(|| q.get("tail"))
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50)
        .clamp(1, RUN_REQ_MAX);
    let from = match q.get("from").and_then(|v| v.parse::<usize>().ok()) {
        Some(f) => f.max(first),
        None => total.saturating_sub(count - 1).max(first),
    };
    let to = (from + count - 1).min(total);
    if from > total {
        return format!("# lines {total}-{total} of {total} ({state}) - from beyond end");
    }
    let mut out = format!("# lines {from}-{to} of {total} ({state})\n");
    for n in from..=to {
        out.push_str(&log.buf[n - first]);
        out.push('\n');
    }
    out
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

fn send_to_tile(tile_id: &str, text: &str) -> Result<(), String> {
    let msg = text.trim_end_matches(['\r', '\n']).to_string();
    let session = sessions().lock().unwrap().get(tile_id).cloned();
    let live = session.is_some()
        || host_handle()
            .session_info(tile_id)
            .map(|(alive, _)| alive)
            .unwrap_or(false);
    if !live {
        return Err("no session for tile".into());
    }
    let bracketed = session
        .as_ref()
        .and_then(|s| s.parser.lock().ok().map(|p| p.screen().bracketed_paste()))
        .unwrap_or(false);
    if bracketed {
        host_handle().write(tile_id, format!("\x1b[200~{msg}\x1b[201~").as_bytes());
    } else {
        host_handle().write(tile_id, msg.as_bytes());
    }
    if let Some(s) = &session {
        s.scrollback.store(0, Ordering::Relaxed);
        s.dirty.store(true, Ordering::Relaxed);
    }
    let tile_id = tile_id.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        host_handle().write(&tile_id, b"\r");
        if let Some(s) = sessions().lock().unwrap().get(&tile_id) {
            s.dirty.store(true, Ordering::Relaxed);
        }
    });
    Ok(())
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
                host_handle().write(&s.tile_id, d.as_bytes());
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
        Some("visible") => {
            let visible = v.get("visible").and_then(|v| v.as_bool()).unwrap_or(true);
            s.visible.store(visible, Ordering::Relaxed);
            if visible {
                s.dirty.store(true, Ordering::Relaxed);
            }
        }
        Some("dismissAgent") => clear_binding_session(&s.tile_id),
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

    let mut tick = tokio::time::interval(Duration::from_millis(60));
    let mut claude = ClaudeTracker::default();
    let mut claude_tick = tokio::time::interval(Duration::from_millis(800));
    let mut last_frame = tokio::time::Instant::now();
    const UNFOCUSED_FRAME_MS: u64 = 250;
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
                if session.visible.load(Ordering::Relaxed)
                    && session.dirty.load(Ordering::Relaxed)
                    && (session.focused.load(Ordering::Relaxed)
                        || last_frame.elapsed() >= Duration::from_millis(UNFOCUSED_FRAME_MS))
                {
                    session.dirty.store(false, Ordering::Relaxed);
                    last_frame = tokio::time::Instant::now();
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
                        host_handle().write(&session.tile_id, &b);
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
            elevated: q.get("elevated").map(|v| v == "1").unwrap_or(false),
            spawn_cmd: None,
            run_kind: "run".into(),
            attach_only: q.get("attach").map(|v| v == "1").unwrap_or(false),
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
            Some(s) => (host_handle().snapshot(&tid), s.cols.load(Ordering::Relaxed)),
            None => match host_handle().session_info(&tid) {
                Some((true, cols)) => (host_handle().snapshot(&tid), cols),
                _ => (load_buffer(&tid), 80),
            },
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

    if path_only == "/agent/send" {
        let q = parse_query(query);
        let tid = q.get("tileId").cloned().unwrap_or_default();
        let len = headers
            .get("content-length")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let mut text = String::new();
        if len > 0 && len <= STATUS_POST_MAX {
            let mut body = vec![0u8; len];
            if stream.read_exact(&mut body).await.is_ok() {
                text = String::from_utf8_lossy(&body).into_owned();
            }
        }
        let body = if text.trim().is_empty() {
            serde_json::json!({ "error": "empty message body" }).to_string()
        } else {
            match send_to_tile(&tid, &text) {
                Ok(()) => "{\"ok\":true}".to_string(),
                Err(e) => serde_json::json!({ "error": e }).to_string(),
            }
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
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
                let pid = s.pid;
                let shell = s.shell.clone();
                tokio::task::spawn_blocking(move || {
                    foreground_command(pid)
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

    if path_only == "/run/start" {
        let q = parse_query(query);
        let cwd = q.get("cwd").cloned().unwrap_or_default();
        let cmd = q.get("cmd").cloned().unwrap_or_default();
        let tile = q.get("tile").cloned().unwrap_or_default();
        let kind = match q.get("kind").map(String::as_str) {
            Some("build") => "build".to_string(),
            _ => "run".to_string(),
        };
        let body = if cwd.is_empty() || cmd.is_empty() || tile.is_empty() {
            serde_json::json!({ "error": "cwd, cmd and tile required" }).to_string()
        } else {
            let key = run_key(&kind, &tile);
            let existing = run_lookup_tile(&kind, &tile);
            match existing {
                Some(s) if !s.exited.load(Ordering::Relaxed) => {
                    serde_json::json!({ "error": "already running" }).to_string()
                }
                _ => {
                    sessions().lock().unwrap().remove(&key);
                    let params = Params {
                        tile_id: key,
                        cols: 120,
                        rows: 30,
                        cwd: Some(cwd.clone()),
                        shell: None,
                        target: None,
                        elevated: false,
                        spawn_cmd: Some(cmd),
                        run_kind: kind.clone(),
                        attach_only: false,
                    };
                    match get_or_create(&params).await {
                        Ok(_) => run_status_json(run_lookup_tile(&kind, &tile)),
                        Err(e) => serde_json::json!({ "error": e }).to_string(),
                    }
                }
            }
        };
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }

    if path_only == "/run/status" {
        let q = parse_query(query);
        let kind = q.get("kind").map(String::as_str).unwrap_or("run");
        let body = run_status_json(run_lookup(
            kind,
            q.get("tile").map(String::as_str),
            q.get("cwd").map(String::as_str),
        ));
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }

    if path_only == "/run/logs" {
        let q = parse_query(query);
        let kind = q.get("kind").map(String::as_str).unwrap_or("run");
        let body = run_logs_text(
            run_lookup(kind, q.get("tile").map(String::as_str), q.get("cwd").map(String::as_str)),
            &q,
        );
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }

    if path_only == "/run/stop" {
        let q = parse_query(query);
        let hard = q.get("hard").map(|v| v == "1").unwrap_or(false);
        let kind = q.get("kind").map(String::as_str).unwrap_or("run");
        if let Some(s) = run_lookup(
            kind,
            q.get("tile").map(String::as_str),
            q.get("cwd").map(String::as_str),
        ) {
            if hard {
                kill_session(&s);
            } else if !s.exited.load(Ordering::Relaxed) {
                run_stop(&s);
            } else {
                sessions().lock().unwrap().remove(&s.tile_id);
            }
        }
        let body = "{\"ok\":true}";
        let resp = format!(
            "HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
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
    if std::env::args().nth(1).as_deref() == Some("host") {
        supervise_brain();
        host::run_host_server(host_port());
        return;
    }
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
    #[cfg(windows)]
    if std::env::args().nth(1).as_deref() != Some(DAEMON_ARG) {
        daemonize();
    }
    if let Some(path) = fresh_path() {
        std::env::set_var("PATH", path);
    }
    tokio::task::block_in_place(|| reconnect_sessions());
    install_claude_hook();
    let listener = TcpListener::bind(("127.0.0.1", port()))
        .await
        .expect("bind sidecar port");
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
        agent_event_ws_msg, decode_osc52, default_status_line, diff_lines, sanitize_event_text,
        status_ws_msg, utf8_valid_len, Arc, AtomicBool, CwdSink, Ordering, RunLog,
        OSC_BG_RESPONSE, OSC_FG_RESPONSE,
    };

    #[test]
    fn run_log_strips_ansi_and_paginates() {
        let mut log = RunLog::new();
        log.feed("\x1b[32mready\x1b[0m in 300ms\r\n");
        log.feed("spinner frame\rLocal: http://localhost:5173/\r");
        log.feed("\n\x1b]0;title\x07plain\npartial");
        assert_eq!(log.total, 3);
        assert_eq!(log.buf[0], "ready in 300ms");
        assert_eq!(log.buf[1], "Local: http://localhost:5173/");
        assert_eq!(log.buf[2], "plain");
        assert_eq!(log.partial, "partial");
        assert_eq!(log.range(), (1, 3));
    }

    #[test]
    fn diff_lines_trims_common_context() {
        assert_eq!(diff_lines("a\nb\nc", "a\nX\nc"), (1, 1));
        assert_eq!(diff_lines("", "a\nb"), (2, 0));
        assert_eq!(diff_lines("a\nb", ""), (0, 2));
        assert_eq!(diff_lines("a\nb\nc", "a\nb\nc"), (0, 0));
        assert_eq!(diff_lines("a\nb", "a\nb\nc\nd"), (2, 0));
    }

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
    fn session_clear_resets_counters() {
        let mut t = super::ClaudeTracker::default();
        t.ingest(
            r#"{"message":{"role":"assistant","usage":{"input_tokens":10},"content":[{"type":"tool_use","name":"Write","input":{"content":"a\nb\nc"}}]}}"#,
        );
        assert_eq!(t.tokens, Some(10));
        assert_eq!(t.added, 3);
        t.ingest(r#"{"type":"attachment","attachment":{"hookName":"SessionStart:clear"}}"#);
        assert_eq!(t.tokens, None);
        assert_eq!(t.added, 0);
        assert_eq!(t.removed, 0);
        assert!(t.reset);
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
    fn raw_ring_monotonic_offsets() {
        let mut r = super::RawRing::new(vec![1, 2, 3]);
        assert_eq!(r.len_total(), 3);
        assert_eq!(r.base, 0);
        assert_eq!(r.snapshot(), vec![1, 2, 3]);

        let fill = vec![0u8; super::RAW_CAP - 3];
        r.push(&fill);
        assert_eq!(r.len_total(), super::RAW_CAP as u64);
        assert_eq!(r.base, 0);

        r.push(&[9, 8, 7]);
        assert_eq!(r.base, 3);
        assert_eq!(r.len_total(), super::RAW_CAP as u64 + 3);
        assert_eq!(r.buf.len(), super::RAW_CAP);
        assert_eq!(&r.buf[super::RAW_CAP - 3..], &[9, 8, 7]);

        let (start, bytes) = r.since(0);
        assert_eq!(start, 3);
        assert_eq!(bytes.len(), super::RAW_CAP);

        let (start2, bytes2) = r.since(3);
        assert_eq!(start2, 3);
        assert_eq!(bytes2.len(), super::RAW_CAP);

        let mid = super::RAW_CAP as u64;
        let (start3, bytes3) = r.since(mid);
        assert_eq!(start3, mid);
        assert_eq!(bytes3, vec![9, 8, 7]);

        let total = r.len_total();
        let (start4, bytes4) = r.since(total);
        assert_eq!(start4, total);
        assert!(bytes4.is_empty());

        let (start5, bytes5) = r.since(total + 99);
        assert_eq!(start5, total);
        assert!(bytes5.is_empty());
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
