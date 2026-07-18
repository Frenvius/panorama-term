use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::RawRing;

pub const KIND_RPC: u8 = 0x01;
pub const KIND_RPC_REPLY: u8 = 0x02;
pub const KIND_DATA: u8 = 0x03;
pub const KIND_INPUT: u8 = 0x04;
pub const KIND_EVENT: u8 = 0x05;

// ---- Frame codec ----

pub fn encode(kind: u8, payload: &[u8]) -> Vec<u8> {
    let len = (1 + payload.len()) as u32;
    let mut out = Vec::with_capacity(5 + payload.len());
    out.extend_from_slice(&len.to_be_bytes());
    out.push(kind);
    out.extend_from_slice(payload);
    out
}

pub fn decode(buf: &[u8]) -> Option<(u8, Vec<u8>, usize)> {
    if buf.len() < 5 {
        return None;
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len == 0 {
        return None;
    }
    let total = 4 + len;
    if buf.len() < total {
        return None;
    }
    let kind = buf[4];
    Some((kind, buf[5..total].to_vec(), total))
}

pub fn encode_data_payload(key: &str, offset: u64, bytes: &[u8]) -> Vec<u8> {
    let kb = key.as_bytes();
    let mut out = Vec::with_capacity(4 + kb.len() + 8 + bytes.len());
    out.extend_from_slice(&(kb.len() as u32).to_be_bytes());
    out.extend_from_slice(kb);
    out.extend_from_slice(&offset.to_be_bytes());
    out.extend_from_slice(bytes);
    out
}

pub fn decode_data_payload(payload: &[u8]) -> Option<(String, u64, Vec<u8>)> {
    if payload.len() < 4 {
        return None;
    }
    let klen = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    if payload.len() < 4 + klen + 8 {
        return None;
    }
    let key = std::str::from_utf8(&payload[4..4 + klen]).ok()?.to_string();
    let off_bytes: [u8; 8] = payload[4 + klen..4 + klen + 8].try_into().ok()?;
    let offset = u64::from_be_bytes(off_bytes);
    Some((key, offset, payload[4 + klen + 8..].to_vec()))
}

pub fn encode_input_payload(key: &str, bytes: &[u8]) -> Vec<u8> {
    let kb = key.as_bytes();
    let mut out = Vec::with_capacity(4 + kb.len() + bytes.len());
    out.extend_from_slice(&(kb.len() as u32).to_be_bytes());
    out.extend_from_slice(kb);
    out.extend_from_slice(bytes);
    out
}

pub fn decode_input_payload(payload: &[u8]) -> Option<(String, Vec<u8>)> {
    if payload.len() < 4 {
        return None;
    }
    let klen = u32::from_be_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;
    if payload.len() < 4 + klen {
        return None;
    }
    let key = std::str::from_utf8(&payload[4..4 + klen]).ok()?.to_string();
    Some((key, payload[4 + klen..].to_vec()))
}

// ---- Session internals ----

struct SubEntry {
    min_offset: u64,
    tx: Sender<Vec<u8>>,
}

struct SessionState {
    ring: RawRing,
    subs: Vec<SubEntry>,
}

struct HostSession {
    key: String,
    pid: u32,
    alive: Arc<AtomicBool>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    _child: Mutex<Box<dyn portable_pty::Child + Send + Sync>>,
    state: Arc<Mutex<SessionState>>,
}

// ---- Host ----

struct HostInner {
    sessions: Mutex<HashMap<String, Arc<HostSession>>>,
}

pub struct Host {
    inner: Arc<HostInner>,
}

impl Host {
    pub fn new() -> Self {
        Host {
            inner: Arc::new(HostInner {
                sessions: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn start(
        self,
        inbound: Receiver<Vec<u8>>,
        outbound: Sender<Vec<u8>>,
    ) -> std::thread::JoinHandle<()> {
        let inner = self.inner;
        std::thread::spawn(move || run_dispatch(inner, inbound, outbound))
    }
}

fn run_dispatch(inner: Arc<HostInner>, inbound: Receiver<Vec<u8>>, outbound: Sender<Vec<u8>>) {
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let bytes = match inbound.recv() {
            Ok(b) => b,
            Err(_) => break,
        };
        pending.extend_from_slice(&bytes);
        while let Some((kind, payload, consumed)) = decode(&pending) {
            pending.drain(0..consumed);
            handle_frame(&inner, kind, &payload, &outbound);
        }
    }
}

fn handle_frame(inner: &Arc<HostInner>, kind: u8, payload: &[u8], outbound: &Sender<Vec<u8>>) {
    match kind {
        KIND_RPC => {
            let v: serde_json::Value = match serde_json::from_slice(payload) {
                Ok(v) => v,
                Err(_) => return,
            };
            let id = v.get("id").and_then(|i| i.as_u64()).unwrap_or(0);
            let op = v.get("op").and_then(|o| o.as_str()).unwrap_or("").to_string();
            let result = dispatch_rpc(inner, &op, &v, outbound);
            let reply = match result {
                Ok(mut obj) => {
                    obj["id"] = serde_json::json!(id);
                    obj["ok"] = serde_json::json!(true);
                    obj
                }
                Err(e) => serde_json::json!({"id": id, "ok": false, "err": e}),
            };
            let _ = outbound.send(encode(KIND_RPC_REPLY, reply.to_string().as_bytes()));
        }
        KIND_INPUT => {
            if let Some((key, bytes)) = decode_input_payload(payload) {
                let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(s) = sess.get(&key).cloned() {
                    drop(sess);
                    let _ = s
                        .writer
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .write_all(&bytes);
                }
            }
        }
        _ => {}
    }
}

fn dispatch_rpc(
    inner: &Arc<HostInner>,
    op: &str,
    v: &serde_json::Value,
    outbound: &Sender<Vec<u8>>,
) -> Result<serde_json::Value, String> {
    match op {
        "spawn" => {
            let key = v["key"].as_str().ok_or("missing key")?.to_string();
            let cwd = v["cwd"].as_str().map(|s| s.to_string());
            let cols = v["cols"].as_u64().unwrap_or(80) as u16;
            let rows = v["rows"].as_u64().unwrap_or(24) as u16;
            let shell = v["shell"].as_str().map(|s| s.to_string());
            let spawn_cmd = v["spawn_cmd"].as_str().map(|s| s.to_string());
            let pid = host_spawn(
                inner,
                key,
                cwd,
                cols.max(2),
                rows.max(2),
                shell,
                spawn_cmd,
                outbound.clone(),
            )?;
            Ok(serde_json::json!({ "pid": pid }))
        }
        "subscribe" => {
            let key = v["key"].as_str().ok_or("missing key")?.to_string();
            let from_offset = v["from_offset"].as_u64().unwrap_or(0);
            let (base, total) = host_subscribe(inner, &key, from_offset, outbound.clone())?;
            Ok(serde_json::json!({ "base": base, "total": total }))
        }
        "unsubscribe" => {
            let key = v["key"].as_str().ok_or("missing key")?;
            let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(s) = sess.get(key) {
                s.state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .subs
                    .clear();
            }
            Ok(serde_json::json!({}))
        }
        "resize" => {
            let key = v["key"].as_str().ok_or("missing key")?;
            let cols = v["cols"].as_u64().unwrap_or(80) as u16;
            let rows = v["rows"].as_u64().unwrap_or(24) as u16;
            let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let s = sess.get(key).ok_or("session not found")?;
            let _ = s
                .master
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            Ok(serde_json::json!({}))
        }
        "kill" => {
            let key = v["key"].as_str().ok_or("missing key")?;
            let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let s = sess.get(key).ok_or("session not found")?;
            let _ = s
                .master
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .resize(PtySize {
                    rows: 1,
                    cols: 1,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            Ok(serde_json::json!({}))
        }
        "list" => {
            let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let list: Vec<serde_json::Value> = sess
                .values()
                .map(|s| {
                    let total = s
                        .state
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .ring
                        .len_total();
                    serde_json::json!({
                        "key": s.key,
                        "pid": s.pid,
                        "alive": s.alive.load(Ordering::Relaxed),
                        "total": total,
                    })
                })
                .collect();
            Ok(serde_json::json!({ "sessions": list }))
        }
        _ => Err(format!("unknown op: {op}")),
    }
}

fn host_spawn(
    inner: &Arc<HostInner>,
    key: String,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    shell: Option<String>,
    spawn_cmd: Option<String>,
    outbound: Sender<Vec<u8>>,
) -> Result<u32, String> {
    let cwd = cwd
        .or_else(|| std::env::var("USERPROFILE").ok())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".into());

    let shell = shell.unwrap_or_else(|| {
        if cfg!(windows) {
            std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into())
        }
    });

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("openpty: {e}"))?;

    let mut cmd = if let Some(ref run_cmd) = spawn_cmd {
        if cfg!(windows) {
            let mut c = CommandBuilder::new("powershell.exe");
            c.arg("-NoLogo");
            c.arg("-Command");
            c.arg(run_cmd);
            c
        } else {
            let mut c = CommandBuilder::new("/bin/sh");
            c.arg("-lc");
            c.arg(run_cmd);
            c
        }
    } else {
        CommandBuilder::new(&shell)
    };
    cmd.cwd(&cwd);
    cmd.env("TERM", "xterm-256color");
    cmd.env("PANORAMA_TERMINAL", "1");

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn: {e}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("clone reader: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("take writer: {e}"))?;
    let pid = child.process_id().unwrap_or(0);
    drop(pair.slave);

    let alive = Arc::new(AtomicBool::new(true));
    let state = Arc::new(Mutex::new(SessionState {
        ring: RawRing::new(Vec::new()),
        subs: Vec::new(),
    }));

    let session = Arc::new(HostSession {
        key: key.clone(),
        pid,
        alive: alive.clone(),
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        _child: Mutex::new(child),
        state: state.clone(),
    });

    inner
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.clone(), session);

    {
        let state = state.clone();
        let alive = alive.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                        let pre = st.ring.len_total();
                        st.ring.push(chunk);
                        let frame =
                            encode(KIND_DATA, &encode_data_payload(&key, pre, chunk));
                        st.subs.retain(|sub| {
                            if pre >= sub.min_offset {
                                sub.tx.send(frame.clone()).is_ok()
                            } else {
                                true
                            }
                        });
                    }
                }
            }
            alive.store(false, Ordering::Relaxed);
            state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .subs
                .clear();
            let event =
                serde_json::json!({ "kind": "exit", "key": key }).to_string();
            let _ = outbound.send(encode(KIND_EVENT, event.as_bytes()));
        });
    }

    Ok(pid)
}

fn host_subscribe(
    inner: &Arc<HostInner>,
    key: &str,
    from_offset: u64,
    outbound: Sender<Vec<u8>>,
) -> Result<(u64, u64), String> {
    let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
    let s = sess.get(key).ok_or("session not found")?.clone();
    drop(sess);

    let (replay_start, replay_bytes, replay_end, base, sub_rx) = {
        let mut st = s.state.lock().unwrap_or_else(|e| e.into_inner());
        let base = st.ring.base;
        let (start, bytes) = st.ring.since(from_offset);
        let end = start + bytes.len() as u64;
        let sub_rx = if s.alive.load(Ordering::Relaxed) {
            let (tx, rx) = channel::<Vec<u8>>();
            st.subs.push(SubEntry { min_offset: end, tx });
            Some(rx)
        } else {
            None
        };
        (start, bytes, end, base, sub_rx)
    };

    if !replay_bytes.is_empty() {
        let _ = outbound.send(encode(
            KIND_DATA,
            &encode_data_payload(key, replay_start, &replay_bytes),
        ));
    }

    if let Some(rx) = sub_rx {
        std::thread::spawn(move || {
            while let Ok(frame) = rx.recv() {
                if outbound.send(frame).is_err() {
                    break;
                }
            }
        });
    }

    Ok((base, replay_end))
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    #[test]
    fn frame_codec_round_trip() {
        for kind in [KIND_RPC, KIND_RPC_REPLY, KIND_DATA, KIND_INPUT, KIND_EVENT] {
            let payload = b"hello world";
            let encoded = encode(kind, payload);
            let (k, p, consumed) = decode(&encoded).unwrap();
            assert_eq!(k, kind);
            assert_eq!(p, payload);
            assert_eq!(consumed, encoded.len());
        }
        assert_eq!(decode(&[0, 0, 0, 0, 0x01]), None);
        assert_eq!(decode(&[0, 0, 0, 3]), None);
    }

    #[test]
    fn data_payload_round_trip() {
        let payload = encode_data_payload("my-key", 1234567890u64, b"raw bytes");
        let (key, offset, bytes) = decode_data_payload(&payload).unwrap();
        assert_eq!(key, "my-key");
        assert_eq!(offset, 1234567890u64);
        assert_eq!(bytes, b"raw bytes");
    }

    #[test]
    fn input_payload_round_trip() {
        let payload = encode_input_payload("tile-42", b"\x1b[A");
        let (key, bytes) = decode_input_payload(&payload).unwrap();
        assert_eq!(key, "tile-42");
        assert_eq!(bytes, b"\x1b[A");
    }

    #[test]
    fn data_payload_empty_bytes() {
        let payload = encode_data_payload("k", 0, b"");
        let (key, offset, bytes) = decode_data_payload(&payload).unwrap();
        assert_eq!(key, "k");
        assert_eq!(offset, 0);
        assert!(bytes.is_empty());
    }

    fn poll_frame(rx: &std::sync::mpsc::Receiver<Vec<u8>>) -> Option<(u8, Vec<u8>)> {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(frame) => decode(&frame).map(|(k, p, _)| (k, p)),
            Err(_) => None,
        }
    }

    #[test]
    fn host_pty_smoke() {
        let (in_tx, in_rx) = channel::<Vec<u8>>();
        let (out_tx, out_rx) = channel::<Vec<u8>>();

        let host = Host::new();
        let _handle = host.start(in_rx, out_tx);

        let spawn_cmd = if cfg!(windows) {
            "cmd /c echo panorama-marker"
        } else {
            "echo panorama-marker"
        };

        let spawn_rpc = serde_json::json!({
            "id": 1u64, "op": "spawn", "key": "smoke",
            "cwd": std::env::temp_dir().to_string_lossy(),
            "cols": 80u64, "rows": 24u64, "spawn_cmd": spawn_cmd,
        });
        in_tx.send(encode(KIND_RPC, spawn_rpc.to_string().as_bytes())).unwrap();

        // Single collection loop - never discard frames; send subscribe reactively.
        let test_deadline = Instant::now() + Duration::from_secs(20);
        let mut spawn_ok = false;
        let mut sub_sent = false;
        let mut sub_replied = false;
        let mut found_marker = false;
        let mut exited = false;
        let mut all_data: Vec<u8> = Vec::new();

        while Instant::now() < test_deadline {
            let remaining = test_deadline
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::from_millis(50))
                .min(Duration::from_millis(200));
            let frame = match out_rx.recv_timeout(remaining) {
                Ok(f) => f,
                Err(RecvTimeoutError::Timeout) => {
                    if spawn_ok && sub_replied && found_marker && exited {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            };
            let (kind, payload) = match decode(&frame).map(|(k, p, _)| (k, p)) {
                Some(x) => x,
                None => continue,
            };
            match kind {
                KIND_RPC_REPLY => {
                    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                    let id = v["id"].as_u64().unwrap_or(0);
                    if id == 1 && !spawn_ok {
                        assert!(v["ok"].as_bool().unwrap_or(false), "spawn failed: {v}");
                        spawn_ok = true;
                        let sub_rpc = serde_json::json!({
                            "id": 2u64, "op": "subscribe",
                            "key": "smoke", "from_offset": 0u64,
                        });
                        in_tx.send(encode(KIND_RPC, sub_rpc.to_string().as_bytes())).unwrap();
                        sub_sent = true;
                    } else if id == 2 && sub_sent && !sub_replied {
                        assert!(v["ok"].as_bool().unwrap_or(false), "subscribe failed: {v}");
                        sub_replied = true;
                    }
                }
                KIND_DATA => {
                    if let Some((key, _offset, bytes)) = decode_data_payload(&payload) {
                        if key == "smoke" {
                            all_data.extend_from_slice(&bytes);
                            if !found_marker {
                                found_marker = all_data
                                    .windows(b"panorama-marker".len())
                                    .any(|w| w == b"panorama-marker");
                            }
                        }
                    }
                }
                KIND_EVENT => {
                    if let Ok(ev) = serde_json::from_slice::<serde_json::Value>(&payload) {
                        if ev["kind"] == "exit" && ev["key"] == "smoke" {
                            exited = true;
                        }
                    }
                }
                _ => {}
            }
            if spawn_ok && sub_replied && found_marker && exited {
                break;
            }
        }

        assert!(spawn_ok, "spawn reply not received in 20s");
        assert!(sub_replied, "subscribe reply not received");
        assert!(
            found_marker,
            "panorama-marker not found in {} bytes: {:?}",
            all_data.len(),
            String::from_utf8_lossy(&all_data)
        );

        // Get total via list (handles any stray DATA/EVENT frames still in queue)
        let list_rpc = serde_json::json!({ "id": 3u64, "op": "list" });
        in_tx.send(encode(KIND_RPC, list_rpc.to_string().as_bytes())).unwrap();

        let list_deadline = Instant::now() + Duration::from_secs(5);
        let mut total: Option<u64> = None;
        while Instant::now() < list_deadline && total.is_none() {
            if let Some((k, payload)) = poll_frame(&out_rx) {
                if k == KIND_RPC_REPLY {
                    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                    if v["id"] == 3u64 {
                        if let Some(sessions) = v["sessions"].as_array() {
                            for s in sessions {
                                if s["key"] == "smoke" {
                                    total = s["total"].as_u64();
                                }
                            }
                        }
                    }
                }
            }
        }
        let total = total.expect("session total not found via list");
        assert!(total > 0, "total bytes must be > 0");

        // Second subscribe from total/2 - verify offset semantics
        let half = total / 2;
        let sub2_rpc = serde_json::json!({
            "id": 4u64, "op": "subscribe",
            "key": "smoke", "from_offset": half,
        });
        in_tx.send(encode(KIND_RPC, sub2_rpc.to_string().as_bytes())).unwrap();

        let sub2_deadline = Instant::now() + Duration::from_secs(5);
        let mut replay_start: Option<u64> = None;
        let mut replay_bytes: Vec<u8> = Vec::new();
        let mut sub2_replied = false;

        while Instant::now() < sub2_deadline {
            let remaining = sub2_deadline
                .checked_duration_since(Instant::now())
                .unwrap_or(Duration::from_millis(50))
                .min(Duration::from_millis(100));
            let frame = match out_rx.recv_timeout(remaining) {
                Ok(f) => f,
                Err(RecvTimeoutError::Timeout) => {
                    if sub2_replied {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            };
            let (kind, payload) = match decode(&frame).map(|(k, p, _)| (k, p)) {
                Some(x) => x,
                None => continue,
            };
            match kind {
                KIND_RPC_REPLY => {
                    let v: serde_json::Value = serde_json::from_slice(&payload).unwrap();
                    if v["id"] == 4u64 {
                        assert!(v["ok"].as_bool().unwrap_or(false), "sub2 failed: {v}");
                        sub2_replied = true;
                    }
                }
                KIND_DATA => {
                    if let Some((key, offset, bytes)) = decode_data_payload(&payload) {
                        if key == "smoke" {
                            if replay_start.is_none() {
                                replay_start = Some(offset);
                            }
                            replay_bytes.extend_from_slice(&bytes);
                        }
                    }
                }
                _ => {}
            }
        }

        assert!(sub2_replied, "second subscribe reply not received");

        let rs = replay_start.unwrap_or(total);
        assert!(rs >= half, "replay start {rs} < from_offset {half}");

        // Replay tail must equal original data from rs onwards
        let skip = rs as usize;
        if skip <= all_data.len() {
            assert_eq!(
                replay_bytes,
                all_data[skip..],
                "replay bytes mismatch from offset {rs}"
            );
        }
    }
}
