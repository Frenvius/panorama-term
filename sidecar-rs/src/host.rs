use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
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
    cols: u16,
    rows: u16,
    meta: serde_json::Value,
    alive: Arc<AtomicBool>,
    exit_once: Arc<AtomicBool>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    state: Arc<Mutex<SessionState>>,
}

// ---- Host ----

pub(crate) struct HostInner {
    pub(crate) sessions: Mutex<HashMap<String, Arc<HostSession>>>,
    brain_out: Mutex<Option<Sender<Vec<u8>>>>,
    brain_gen: AtomicU64,
}

pub struct Host {
    pub(crate) inner: Arc<HostInner>,
}

impl Clone for Host {
    fn clone(&self) -> Self {
        Host { inner: self.inner.clone() }
    }
}

impl Host {
    pub fn new() -> Self {
        Host {
            inner: Arc::new(HostInner {
                sessions: Mutex::new(HashMap::new()),
                brain_out: Mutex::new(None),
                brain_gen: AtomicU64::new(0),
            }),
        }
    }

    pub fn start(&self, inbound: Receiver<Vec<u8>>, outbound: Sender<Vec<u8>>) -> std::thread::JoinHandle<()> {
        let inner = self.inner.clone();
        std::thread::spawn(move || run_dispatch(inner, inbound, outbound))
    }

    pub fn snapshot(&self, key: &str) -> Vec<u8> {
        let sess = self.inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = sess.get(key) {
            s.state.lock().unwrap_or_else(|e| e.into_inner()).ring.snapshot()
        } else {
            Vec::new()
        }
    }

    pub fn pid_of(&self, key: &str) -> Option<u32> {
        let sess = self.inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
        sess.get(key).map(|s| s.pid)
    }
}

pub fn run_host_server(port: u16) {
    let host = Host::new();
    let listener = TcpListener::bind(("127.0.0.1", port))
        .unwrap_or_else(|e| panic!("host bind {port}: {e}"));
    eprintln!("[host] listening on 127.0.0.1:{port}");
    loop {
        match listener.accept() {
            Ok((stream, addr)) => {
                eprintln!("[host] brain connected from {addr}");
                let h = host.clone();
                std::thread::spawn(move || bridge_connection(h, stream));
            }
            Err(e) => eprintln!("[host] accept error: {e}"),
        }
    }
}

fn bridge_connection(host: Host, stream: TcpStream) {
    let write_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[host] tcp clone failed: {e}");
            return;
        }
    };

    let (in_tx, in_rx) = channel::<Vec<u8>>();
    let (out_tx, out_rx) = channel::<Vec<u8>>();

    host.start(in_rx, out_tx);

    std::thread::spawn(move || {
        let mut ws = write_stream;
        while let Ok(frame) = out_rx.recv() {
            if ws.write_all(&frame).is_err() {
                break;
            }
        }
    });

    let mut rs = stream;
    let mut pending: Vec<u8> = Vec::new();
    let mut buf = [0u8; 65536];
    loop {
        match rs.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                pending.extend_from_slice(&buf[..n]);
                let mut offset = 0;
                while offset + 4 < pending.len() {
                    let len = u32::from_be_bytes([
                        pending[offset],
                        pending[offset + 1],
                        pending[offset + 2],
                        pending[offset + 3],
                    ]) as usize;
                    if len == 0 {
                        break;
                    }
                    let total = 4 + len;
                    if pending.len() < offset + total {
                        break;
                    }
                    let _ = in_tx.send(pending[offset..offset + total].to_vec());
                    offset += total;
                }
                pending.drain(0..offset);
            }
        }
    }
    eprintln!("[host] brain disconnected");
}

fn run_dispatch(inner: Arc<HostInner>, inbound: Receiver<Vec<u8>>, outbound: Sender<Vec<u8>>) {
    let mut my_gen = 0u64;
    let mut pending: Vec<u8> = Vec::new();
    loop {
        let bytes = match inbound.recv() {
            Ok(b) => b,
            Err(_) => break,
        };
        if my_gen == 0 {
            my_gen = inner.brain_gen.fetch_add(1, Ordering::SeqCst) + 1;
            *inner.brain_out.lock().unwrap_or_else(|e| e.into_inner()) = Some(outbound.clone());
        }
        pending.extend_from_slice(&bytes);
        while let Some((kind, payload, consumed)) = decode(&pending) {
            pending.drain(0..consumed);
            handle_frame(&inner, kind, &payload, &outbound);
        }
    }
    if my_gen != 0 && inner.brain_gen.load(Ordering::SeqCst) == my_gen {
        *inner.brain_out.lock().unwrap_or_else(|e| e.into_inner()) = None;
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

fn emit_session_exit(
    exit_once: &Arc<AtomicBool>,
    alive: &Arc<AtomicBool>,
    inner: &Arc<HostInner>,
    key: &str,
    child_arc: &Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
    state: &Arc<Mutex<SessionState>>,
) {
    if exit_once.swap(true, Ordering::AcqRel) {
        return;
    }
    alive.store(false, Ordering::Relaxed);
    state.lock().unwrap_or_else(|e| e.into_inner()).subs.clear();
    let exit_code: Option<u32> = (0..10).find_map(|i| {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        child_arc
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
            .ok()
            .flatten()
            .and_then(|st| Some(st.exit_code()))
    });
    let event = serde_json::json!({
        "kind": "exit",
        "key": key,
        "code": exit_code,
    })
    .to_string();
    let frame = encode(KIND_EVENT, event.as_bytes());
    if let Some(tx) = inner.brain_out.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = tx.send(frame);
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
            let exe = v["exe"].as_str().ok_or("missing exe")?.to_string();
            let args: Vec<String> = v["args"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
                .unwrap_or_default();
            let env: Vec<(String, String)> = v["env"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|pair| {
                            let k = pair.get(0)?.as_str()?;
                            let val = pair.get(1)?.as_str()?;
                            Some((k.to_string(), val.to_string()))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let seed = base64::engine::general_purpose::STANDARD
                .decode(v["seed"].as_str().unwrap_or(""))
                .unwrap_or_default();
            let pid = host_spawn(
                inner,
                key,
                cwd,
                cols.max(2),
                rows.max(2),
                exe,
                args,
                env,
                seed,
                v["meta"].clone(),
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
            let s = sess.get(key).ok_or("session not found")?.clone();
            drop(sess);
            let _ = s.child.lock().unwrap_or_else(|e| e.into_inner()).kill();
            Ok(serde_json::json!({}))
        }
        "snapshot" => {
            let key = v["key"].as_str().ok_or("missing key")?;
            let sess = inner.sessions.lock().unwrap_or_else(|e| e.into_inner());
            let s = sess.get(key).ok_or("session not found")?;
            let st = s.state.lock().unwrap_or_else(|e| e.into_inner());
            let base = st.ring.base;
            let data = st.ring.snapshot();
            let total = base + data.len() as u64;
            drop(st);
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            Ok(serde_json::json!({ "base": base, "total": total, "data": encoded }))
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
                        "cols": s.cols,
                        "rows": s.rows,
                        "alive": s.alive.load(Ordering::Relaxed),
                        "total": total,
                        "meta": s.meta,
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
    exe: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    seed: Vec<u8>,
    meta: serde_json::Value,
    _outbound: Sender<Vec<u8>>,
) -> Result<u32, String> {
    let cwd = cwd
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

    let mut cmd = CommandBuilder::new(&exe);
    for arg in &args {
        cmd.arg(arg);
    }
    for (k, val) in &env {
        cmd.env(k, val);
    }
    cmd.env_remove("NO_COLOR");
    cmd.cwd(&cwd);

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("spawn '{exe}': {e}"))?;
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
    let exit_once = Arc::new(AtomicBool::new(false));
    let state = Arc::new(Mutex::new(SessionState {
        ring: RawRing::new(seed),
        subs: Vec::new(),
    }));
    let child_arc = Arc::new(Mutex::new(child));

    let session = Arc::new(HostSession {
        key: key.clone(),
        pid,
        cols,
        rows,
        meta,
        alive: alive.clone(),
        exit_once: exit_once.clone(),
        writer: Mutex::new(writer),
        master: Mutex::new(pair.master),
        child: child_arc.clone(),
        state: state.clone(),
    });

    inner
        .sessions
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key.clone(), session);

    {
        let inner_r = inner.clone();
        let alive_r = alive.clone();
        let exit_once_r = exit_once.clone();
        let state_r = state.clone();
        let child_r = child_arc.clone();
        let key_r = key.clone();
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        let mut st = state_r.lock().unwrap_or_else(|e| e.into_inner());
                        let pre = st.ring.len_total();
                        st.ring.push(chunk);
                        let frame =
                            encode(KIND_DATA, &encode_data_payload(&key_r, pre, chunk));
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
            emit_session_exit(
                &exit_once_r,
                &alive_r,
                &inner_r,
                &key_r,
                &child_r,
                &state_r,
            );
        });
    }

    {
        let inner_w = inner.clone();
        let alive_w = alive.clone();
        let exit_once_w = exit_once.clone();
        let state_w = state.clone();
        let child_w = child_arc.clone();
        let key_w = key.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(300));
            if exit_once_w.load(Ordering::Relaxed) {
                return;
            }
            let exited = {
                let mut c = child_w.lock().unwrap_or_else(|e| e.into_inner());
                c.try_wait().ok().flatten().is_some()
            };
            if exited {
                emit_session_exit(
                    &exit_once_w,
                    &alive_w,
                    &inner_w,
                    &key_w,
                    &child_w,
                    &state_w,
                );
                return;
            }
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

    fn make_spawn_rpc(key: &str, exe: &str, args: &[&str], id: u64) -> Vec<u8> {
        let rpc = serde_json::json!({
            "id": id,
            "op": "spawn",
            "key": key,
            "cwd": std::env::temp_dir().to_string_lossy(),
            "cols": 80u64,
            "rows": 24u64,
            "exe": exe,
            "args": args,
            "env": [["TERM", "xterm-256color"]],
            "seed": "",
        });
        encode(KIND_RPC, rpc.to_string().as_bytes())
    }

    #[test]
    fn tcp_bridge_framing() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        let host = Host::new();

        std::thread::spawn(move || {
            if let Ok((conn, _)) = listener.accept() {
                bridge_connection(host, conn);
            }
        });

        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut client_r = client.try_clone().unwrap();

        let list_rpc = serde_json::json!({"id": 1u64, "op": "list"});
        let frame = encode(KIND_RPC, list_rpc.to_string().as_bytes());
        let mut client_w = client;
        client_w.write_all(&frame).unwrap();

        let mut pending: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut got_reply = false;
        while Instant::now() < deadline {
            client_r.set_read_timeout(Some(Duration::from_millis(200))).ok();
            match client_r.read(&mut buf) {
                Ok(0) | Err(_) => std::thread::sleep(Duration::from_millis(10)),
                Ok(n) => {
                    pending.extend_from_slice(&buf[..n]);
                    if let Some((k, p, _)) = decode(&pending) {
                        if k == KIND_RPC_REPLY {
                            let v: serde_json::Value = serde_json::from_slice(&p).unwrap();
                            assert_eq!(v["id"], 1u64);
                            assert!(v["ok"].as_bool().unwrap_or(false));
                            assert!(v["sessions"].as_array().is_some());
                            got_reply = true;
                            break;
                        }
                    }
                }
            }
        }
        assert!(got_reply, "did not receive list reply over TCP");
    }

    #[test]
    fn host_pty_smoke() {
        let (in_tx, in_rx) = channel::<Vec<u8>>();
        let (out_tx, out_rx) = channel::<Vec<u8>>();

        let host = Host::new();
        let _handle = host.start(in_rx, out_tx);

        let (exe, args): (&str, &[&str]) = if cfg!(windows) {
            ("cmd.exe", &["/c", "echo panorama-marker"])
        } else {
            ("/bin/sh", &["-c", "echo panorama-marker"])
        };

        in_tx.send(make_spawn_rpc("smoke", exe, args, 1)).unwrap();

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
                            "id": 2u64,
                            "op": "subscribe",
                            "key": "smoke",
                            "from_offset": 0u64,
                        });
                        in_tx
                            .send(encode(KIND_RPC, sub_rpc.to_string().as_bytes()))
                            .unwrap();
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

        let list_rpc = serde_json::json!({ "id": 3u64, "op": "list" });
        in_tx
            .send(encode(KIND_RPC, list_rpc.to_string().as_bytes()))
            .unwrap();

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

        let half = total / 2;
        let sub2_rpc = serde_json::json!({
            "id": 4u64,
            "op": "subscribe",
            "key": "smoke",
            "from_offset": half,
        });
        in_tx
            .send(encode(KIND_RPC, sub2_rpc.to_string().as_bytes()))
            .unwrap();

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
