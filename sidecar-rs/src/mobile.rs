use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use crypto_secretbox::aead::{Aead, KeyInit};
use crypto_secretbox::{Nonce, XSalsa20Poly1305};
use futures_util::{Sink, SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use x25519_dalek::{PublicKey, StaticSecret};

mod relay;

use super::{
    activity, attach, build_frame, chat, files, hidden_command, find_transcript, forget_session, git_glance, panorama_dir, read_binding_rec, reflow,
    run_commands, run_lookup_tile, run_start_json, run_status_json, run_stop, sessions, ui, urlenc,
    write_atomic, write_input, Session,
};

const PROTOCOL_VERSION: u64 = 1;
const PAIRING_TTL: Duration = Duration::from_secs(300);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
const FRAME_INTERVAL: Duration = Duration::from_millis(120);
const TEXT_ROWS: usize = 1000;
const SNIPPET_ROWS: usize = 60;
const MAX_MESSAGE: usize = 1 << 20;
const KIND_JSON: u8 = 0;
const KIND_FRAME: u8 = 1;
const DIR_CLIENT: u8 = 0;
const DIR_SERVER: u8 = 1;
const PUSH_URL: &str = "https://exp.host/--/api/v2/push/send";
const PUSH_CHANNEL: &str = "agent";
const PUSH_TIMEOUT: Duration = Duration::from_secs(10);
const PUSH_TOKEN_PREFIXES: &[&str] = &["ExponentPushToken[", "ExpoPushToken["];
const LOCAL_ORIGINS: &[&str] = &[
    "tauri://localhost",
    "http://tauri.localhost",
    "https://tauri.localhost",
    "http://localhost:1471",
    "http://127.0.0.1:1471",
];

type Tx = Pin<Box<dyn Sink<Message, Error = WsError> + Send>>;
type Rx = Pin<Box<dyn Stream<Item = Result<Message, WsError>> + Send>>;

static LISTENING: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct Device {
    id: String,
    name: String,
    public_key: [u8; 32],
    paired_at: u64,
    push_token: Option<String>,
}

struct State {
    enabled: bool,
    remote: bool,
    secret: StaticSecret,
    devices: Vec<Device>,
    pairing: Option<(String, Instant)>,
    listener: Option<tokio::task::AbortHandle>,
    relay: Option<tokio::task::AbortHandle>,
}

fn port() -> u16 {
    std::env::var("PANORAMA_MOBILE_PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(if cfg!(debug_assertions) { 9879 } else { 9779 })
}

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::getrandom(&mut buf).expect("os randomness");
    buf
}

fn decode_fixed<const N: usize>(raw: &str) -> Option<[u8; N]> {
    b64().decode(raw).ok()?.try_into().ok()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn command_line(program: &str, args: &[&str]) -> Option<String> {
    let out = hidden_command(program).args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

// GUI apps on macOS do not inherit HOSTNAME, so ask the system for the friendly name.
fn host_name() -> String {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(|| {
        std::env::var("COMPUTERNAME")
            .ok()
            .or_else(|| command_line("scutil", &["--get", "ComputerName"]))
            .or_else(|| command_line("hostname", &[]))
            .unwrap_or_else(|| "Panorama".into())
    })
    .clone()
}

fn state_path() -> Option<PathBuf> {
    Some(panorama_dir()?.join("mobile.json"))
}

fn load_state() -> State {
    let saved: Value = state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(Value::Null);
    let stored_secret = saved["secretKey"].as_str().and_then(decode_fixed::<32>);
    let devices = saved["devices"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|d| {
                    Some(Device {
                        id: d["id"].as_str()?.to_string(),
                        name: d["name"].as_str().unwrap_or("Phone").to_string(),
                        public_key: decode_fixed(d["publicKey"].as_str()?)?,
                        paired_at: d["pairedAt"].as_u64().unwrap_or(0),
                        push_token: d["pushToken"].as_str().map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let state = State {
        enabled: saved["enabled"].as_bool().unwrap_or(false),
        remote: saved["remote"].as_bool().unwrap_or(true),
        secret: StaticSecret::from(stored_secret.unwrap_or_else(random_bytes)),
        devices,
        pairing: None,
        listener: None,
        relay: None,
    };
    if stored_secret.is_none() {
        save_state(&state);
    }
    state
}

fn save_state(st: &State) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let devices: Vec<Value> = st
        .devices
        .iter()
        .map(|d| {
            json!({
                "id": d.id,
                "name": d.name,
                "publicKey": b64().encode(d.public_key),
                "pairedAt": d.paired_at,
                "pushToken": d.push_token,
            })
        })
        .collect();
    let body = json!({
        "enabled": st.enabled,
        "remote": st.remote,
        "secretKey": b64().encode(st.secret.to_bytes()),
        "devices": devices,
    });
    if let Err(e) = write_atomic(&path, body.to_string().as_bytes()) {
        eprintln!("mobile: save {}: {e}", path.display());
    }
}

fn state() -> MutexGuard<'static, State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE
        .get_or_init(|| Mutex::new(load_state()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn server_public_key(st: &State) -> [u8; 32] {
    PublicKey::from(&st.secret).to_bytes()
}

fn default_route_ip() -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind(("0.0.0.0", 0)).ok()?;
    socket.connect(("8.8.8.8", 53)).ok()?;
    match socket.local_addr().ok()?.ip() {
        std::net::IpAddr::V4(ip) => Some(ip),
        _ => None,
    }
}

fn address_rank(ip: &Ipv4Addr, primary: Option<Ipv4Addr>) -> u8 {
    let [a, b, ..] = ip.octets();
    if Some(*ip) == primary {
        0
    } else if a == 100 && (64..128).contains(&b) {
        1
    } else if ip.is_private() {
        2
    } else {
        3
    }
}

fn lan_addresses() -> Vec<String> {
    let mut ips: Vec<Ipv4Addr> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|iface| match iface.ip() {
            std::net::IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => Some(ip),
            _ => None,
        })
        .collect();
    let primary = default_route_ip();
    ips.sort_by_key(|ip| address_rank(ip, primary));
    ips.dedup();
    ips.into_iter().map(|ip| format!("{ip}:{}", port())).collect()
}

fn start_listener(st: &mut State) {
    if st.listener.is_some() {
        return;
    }
    let bound = std::net::TcpListener::bind(("0.0.0.0", port()))
        .and_then(|l| l.set_nonblocking(true).map(|_| l))
        .and_then(TcpListener::from_std);
    let listener = match bound {
        Ok(l) => l,
        Err(e) => {
            eprintln!("mobile: bind {}: {e}", port());
            return;
        }
    };
    LISTENING.store(true, Ordering::Relaxed);
    let task = tokio::spawn(async move {
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                let _ = stream.set_nodelay(true);
                tokio::spawn(handle_conn(stream));
            }
        }
    });
    st.listener = Some(task.abort_handle());
}

fn stop_listener(st: &mut State) {
    if let Some(task) = st.listener.take() {
        task.abort();
    }
    LISTENING.store(false, Ordering::Relaxed);
}

fn sync_relay(st: &mut State) {
    let wanted = st.enabled && st.remote && !st.devices.is_empty();
    match (wanted, st.relay.take()) {
        (true, Some(task)) => st.relay = Some(task),
        (true, None) => st.relay = Some(relay::spawn(st.secret.clone())),
        (false, Some(task)) => relay::stop(task),
        (false, None) => {}
    }
}

pub fn boot() {
    let mut st = state();
    if st.enabled {
        start_listener(&mut st);
    }
    sync_relay(&mut st);
}

fn state_json(st: &State) -> Value {
    let devices: Vec<Value> = st
        .devices
        .iter()
        .map(|d| json!({ "id": d.id, "name": d.name, "pairedAt": d.paired_at }))
        .collect();
    json!({
        "enabled": st.enabled,
        "remote": st.remote,
        "relay": relay::status(),
        "listening": LISTENING.load(Ordering::Relaxed),
        "port": port(),
        "hostName": host_name(),
        "addresses": lan_addresses(),
        "devices": devices,
    })
}

fn set_enabled(on: bool) -> Value {
    let mut st = state();
    st.enabled = on;
    if on {
        start_listener(&mut st);
    } else {
        st.pairing = None;
        stop_listener(&mut st);
    }
    sync_relay(&mut st);
    save_state(&st);
    state_json(&st)
}

fn set_remote(on: bool) -> Value {
    let mut st = state();
    st.remote = on;
    sync_relay(&mut st);
    save_state(&st);
    state_json(&st)
}

fn start_pairing() -> Value {
    let mut st = state();
    if !st.enabled {
        st.enabled = true;
        save_state(&st);
    }
    start_listener(&mut st);
    let code = hex(&random_bytes::<8>());
    st.pairing = Some((code.clone(), Instant::now() + PAIRING_TTL));
    let hosts = lan_addresses().join(",");
    let uri = format!(
        "panorama-mobile://pair?v={PROTOCOL_VERSION}&k={}&c={code}&n={}&h={}",
        urlenc(&b64().encode(server_public_key(&st))),
        urlenc(&host_name()),
        urlenc(&hosts),
    );
    json!({
        "code": code,
        "uri": uri,
        "expiresAt": now_ms() + PAIRING_TTL.as_millis() as u64,
    })
}

fn revoke(id: &str) -> Value {
    let mut st = state();
    st.devices.retain(|d| d.id != id);
    sync_relay(&mut st);
    save_state(&st);
    state_json(&st)
}

pub fn origin_allowed(origin: Option<&str>) -> bool {
    origin.map_or(true, |o| LOCAL_ORIGINS.contains(&o))
}

pub fn local_route(path: &str, query: &HashMap<String, String>, origin: Option<&str>) -> Option<(u16, String)> {
    let action = path.strip_prefix("/mobile/")?;
    if !origin_allowed(origin) {
        return Some((403, json!({ "error": "origin not allowed" }).to_string()));
    }
    let body = match action {
        "state" => state_json(&state()),
        "enable" => set_enabled(query.get("on").map(|v| v == "1").unwrap_or(false)),
        "remote" => set_remote(query.get("on").map(|v| v == "1").unwrap_or(false)),
        "pair" => start_pairing(),
        "cancel" => {
            let mut st = state();
            st.pairing = None;
            state_json(&st)
        }
        "revoke" => revoke(query.get("id").map(String::as_str).unwrap_or_default()),
        _ => return Some((404, json!({ "error": "unknown mobile route" }).to_string())),
    };
    Some((200, body.to_string()))
}

pub fn cors_origin(origin: Option<&str>) -> &str {
    origin.filter(|o| LOCAL_ORIGINS.contains(o)).unwrap_or("null")
}

struct Channel {
    cipher: XSalsa20Poly1305,
    send_ctr: u64,
    recv_ctr: u64,
}

fn nonce(dir: u8, ctr: u64) -> Nonce {
    let mut n = [0u8; 24];
    n[0] = dir;
    n[1..9].copy_from_slice(&ctr.to_le_bytes());
    Nonce::from(n)
}

impl Channel {
    fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: XSalsa20Poly1305::new(key.into()),
            send_ctr: 0,
            recv_ctr: 0,
        }
    }

    fn seal(&mut self, plain: &[u8]) -> Vec<u8> {
        let sealed = self
            .cipher
            .encrypt(&nonce(DIR_SERVER, self.send_ctr), plain)
            .expect("secretbox encrypt");
        self.send_ctr += 1;
        sealed
    }

    fn open(&mut self, sealed: &[u8]) -> Option<Vec<u8>> {
        let plain = self.cipher.decrypt(&nonce(DIR_CLIENT, self.recv_ctr), sealed).ok()?;
        self.recv_ctr += 1;
        Some(plain)
    }
}

fn session_key(
    secret: &StaticSecret,
    client_pk: &[u8; 32],
    client_nonce: &[u8; 24],
    server_nonce: &[u8; 24],
) -> Option<[u8; 32]> {
    let shared = secret.diffie_hellman(&PublicKey::from(*client_pk));
    if !shared.was_contributory() {
        return None;
    }
    let mut h = Sha512::new();
    h.update(shared.as_bytes());
    h.update(client_pk);
    h.update(PublicKey::from(secret).as_bytes());
    h.update(client_nonce);
    h.update(server_nonce);
    let digest = h.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&digest[..32]);
    Some(key)
}

fn parse_json(plain: &[u8]) -> Option<Value> {
    let (&kind, body) = plain.split_first()?;
    if kind != KIND_JSON {
        return None;
    }
    serde_json::from_slice(body).ok()
}

async fn send_json(tx: &mut Tx, chan: &mut Channel, v: &Value) -> Result<(), ()> {
    let mut plain = vec![KIND_JSON];
    plain.extend_from_slice(v.to_string().as_bytes());
    tx.send(Message::Binary(chan.seal(&plain))).await.map_err(|_| ())
}

async fn send_frame(tx: &mut Tx, chan: &mut Channel, tile_id: &str, frame: &[u8]) -> Result<(), ()> {
    let id = tile_id.as_bytes();
    let mut plain = Vec::with_capacity(3 + id.len() + frame.len());
    plain.push(KIND_FRAME);
    plain.extend_from_slice(&(id.len() as u16).to_le_bytes());
    plain.extend_from_slice(id);
    plain.extend_from_slice(frame);
    tx.send(Message::Binary(chan.seal(&plain))).await.map_err(|_| ())
}

fn codes_match(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn pair_device(public_key: [u8; 32], code: &str, name: &str) -> Option<String> {
    let mut st = state();
    let (expected, expires) = st.pairing.clone()?;
    if Instant::now() > expires || !codes_match(&expected, &code.to_ascii_uppercase()) {
        return None;
    }
    st.pairing = None;
    if let Some(known) = st.devices.iter().find(|d| d.public_key == public_key) {
        return Some(known.id.clone());
    }
    let device = Device {
        id: hex(&random_bytes::<8>()),
        name: name.chars().take(64).collect(),
        public_key,
        paired_at: now_ms(),
        push_token: None,
    };
    let id = device.id.clone();
    st.devices.push(device);
    sync_relay(&mut st);
    save_state(&st);
    Some(id)
}

fn authorize(public_key: &[u8; 32]) -> Option<String> {
    state()
        .devices
        .iter()
        .find(|d| &d.public_key == public_key)
        .map(|d| d.id.clone())
}

fn device_active(id: &str) -> bool {
    let st = state();
    st.enabled && st.devices.iter().any(|d| d.id == id)
}

async fn next_data(rx: &mut Rx) -> Option<Message> {
    loop {
        match rx.next().await?.ok()? {
            Message::Ping(_) | Message::Pong(_) => continue,
            msg => return Some(msg),
        }
    }
}

async fn handshake(tx: &mut Tx, rx: &mut Rx) -> Option<(Channel, String)> {
    let Message::Text(hello) = next_data(rx).await? else {
        return None;
    };
    let hello: Value = serde_json::from_str(&hello).ok()?;
    if hello["t"] != "hello" {
        return None;
    }
    let client_pk: [u8; 32] = decode_fixed(hello["pk"].as_str()?)?;
    let client_nonce: [u8; 24] = decode_fixed(hello["n"].as_str()?)?;
    let server_nonce: [u8; 24] = random_bytes();
    let (secret, server_pk) = {
        let st = state();
        (st.secret.clone(), server_public_key(&st))
    };
    let reply = json!({
        "t": "hello",
        "v": PROTOCOL_VERSION,
        "pk": b64().encode(server_pk),
        "n": b64().encode(server_nonce),
    });
    tx.send(Message::Text(reply.to_string())).await.ok()?;

    let key = session_key(&secret, &client_pk, &client_nonce, &server_nonce)?;
    let mut chan = Channel::new(&key);
    let Message::Binary(first) = next_data(rx).await? else {
        return None;
    };
    let intro = parse_json(&chan.open(&first)?)?;
    let device_id = match intro["t"].as_str() {
        Some("pair") => pair_device(
            client_pk,
            intro["code"].as_str().unwrap_or_default(),
            intro["name"].as_str().unwrap_or("Phone"),
        ),
        Some("auth") => authorize(&client_pk),
        _ => None,
    };
    let Some(device_id) = device_id else {
        let _ = send_json(tx, &mut chan, &json!({ "t": "denied" })).await;
        return None;
    };
    let welcome = json!({
        "t": "welcome",
        "v": PROTOCOL_VERSION,
        "deviceId": device_id,
        "host": host_name(),
    });
    send_json(tx, &mut chan, &welcome).await.ok()?;
    Some((chan, device_id))
}

fn lookup(tile_id: &str) -> Option<Arc<Session>> {
    sessions().lock().unwrap().get(tile_id).cloned()
}

fn tile_kind(tile_id: &str) -> &'static str {
    if tile_id.starts_with("run:") {
        "run"
    } else if tile_id.starts_with("build:") {
        "build"
    } else {
        "terminal"
    }
}

fn shell_snippet(session: &Session) -> Option<Value> {
    let mut parser = session.parser.lock().unwrap_or_else(|e| e.into_inner());
    if parser.screen().alternate_screen() {
        return None;
    }
    let view = reflow::logical_lines(parser.screen_mut(), SNIPPET_ROWS);
    drop(parser);
    let texts: Vec<&str> = view.lines.iter().map(reflow::TextLine::text).collect();
    activity::shell_snippet(&texts)
}

fn tile_snippet(session: &Session, agent: Option<&str>) -> Option<Value> {
    if agent.is_some() {
        return activity::agent_snippet(&session.tile_id);
    }
    if tile_kind(&session.tile_id) != "terminal" {
        return None;
    }
    shell_snippet(session)
}

fn tiles_json() -> Value {
    let list: Vec<Arc<Session>> = sessions().lock().unwrap().values().cloned().collect();
    let tiles: Vec<Value> = list
        .iter()
        .map(|s| {
            let agent = read_binding_rec(&s.tile_id)
                .and_then(|b| b.get("agent").and_then(|a| a.as_str()).map(str::to_string));
            let snippet = tile_snippet(s, agent.as_deref());
            let model = s
                .agent_state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get("model")
                .cloned();
            json!({
                "tileId": s.tile_id,
                "kind": tile_kind(&s.tile_id),
                "title": s.title.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                "cwd": s.cwd.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                "branch": s.branch.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                "alive": !s.exited.load(Ordering::Relaxed),
                "agent": agent,
                "model": model,
                "status": agent_status(&s.tile_id),
                "snippet": snippet,
            })
        })
        .collect();
    Value::Array(tiles)
}

fn str_param<'a>(p: &'a Value, key: &str) -> Result<&'a str, String> {
    p.get(key).and_then(|v| v.as_str()).ok_or_else(|| format!("missing {key}"))
}

fn live_session(tile_id: &str) -> Result<Arc<Session>, String> {
    lookup(tile_id).ok_or_else(|| "no session for tile".to_string())
}

fn agent_statuses() -> MutexGuard<'static, HashMap<String, Value>> {
    static STATUS: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    STATUS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

pub fn record_status(tile_id: &str, status: &Value) {
    if !status.is_string() {
        return;
    }
    if status != "waiting" {
        pending_prompts().remove(tile_id);
    }
    agent_statuses().insert(tile_id.to_string(), status.clone());
}

fn agent_status(tile_id: &str) -> Value {
    agent_statuses().get(tile_id).cloned().unwrap_or(Value::Null)
}

fn pending_prompts() -> MutexGuard<'static, HashMap<String, Value>> {
    static PROMPTS: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    PROMPTS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn pending_prompt(tile_id: &str) -> Value {
    pending_prompts().get(tile_id).cloned().unwrap_or(Value::Null)
}

pub fn agent_event(tile_id: &str, event: &str, evt: &Value) {
    match event {
        "permission" => {
            let tool = evt["tool_name"].as_str().unwrap_or("a tool").to_string();
            let prompt = json!({ "tool": tool, "summary": chat::tool_summary(&evt["tool_input"]) });
            pending_prompts().insert(tile_id.to_string(), prompt);
            notify(tile_id, &format!("Claude wants to use {tool}"));
        }
        "stop" => notify(tile_id, "Claude finished"),
        _ => {}
    }
}

fn is_push_token(token: &str) -> bool {
    token.len() <= 256 && token.ends_with(']') && PUSH_TOKEN_PREFIXES.iter().any(|p| token.starts_with(p))
}

fn register_push(device_id: &str, token: &str) {
    let mut st = state();
    let mut changed = false;
    for device in st.devices.iter_mut() {
        let next = if device.id == device_id {
            Some(token.to_string())
        } else if device.push_token.as_deref() == Some(token) {
            None
        } else {
            device.push_token.clone()
        };
        if device.push_token != next {
            device.push_token = next;
            changed = true;
        }
    }
    if changed {
        save_state(&st);
    }
}

fn drop_push_tokens(tokens: &[String]) {
    let mut st = state();
    for device in st.devices.iter_mut() {
        if device.push_token.as_ref().is_some_and(|t| tokens.contains(t)) {
            device.push_token = None;
        }
    }
    save_state(&st);
}

fn tile_project(tile_id: &str) -> String {
    let cwd = lookup(tile_id)
        .and_then(|s| s.cwd.lock().unwrap_or_else(|e| e.into_inner()).clone())
        .unwrap_or_default();
    std::path::Path::new(&cwd)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Panorama")
        .to_string()
}

fn notify(tile_id: &str, title: &str) {
    let (tokens, host_key) = {
        let st = state();
        if !st.enabled {
            return;
        }
        let tokens: Vec<String> = st.devices.iter().filter_map(|d| d.push_token.clone()).collect();
        (tokens, b64().encode(server_public_key(&st)))
    };
    if tokens.is_empty() {
        return;
    }
    let project = tile_project(tile_id);
    let body = format!("{project} · {}", host_name());
    let messages: Vec<Value> = tokens
        .iter()
        .map(|to| {
            json!({
                "to": to,
                "title": title,
                "body": body,
                "sound": "default",
                "priority": "high",
                "channelId": PUSH_CHANNEL,
                "data": { "hostKey": host_key, "tileId": tile_id, "title": project, "agent": "claude" },
            })
        })
        .collect();
    std::thread::spawn(move || send_push(&messages));
}

fn send_push(messages: &[Value]) {
    let agent = ureq::AgentBuilder::new().timeout(PUSH_TIMEOUT).build();
    let reply = agent
        .post(PUSH_URL)
        .set("accept", "application/json")
        .set("content-type", "application/json")
        .send_string(&Value::from(messages.to_vec()).to_string())
        .map_err(|e| e.to_string())
        .and_then(|res| res.into_string().map_err(|e| e.to_string()));
    let body = match reply {
        Ok(body) => body,
        Err(e) => {
            eprintln!("mobile: push failed: {e}");
            return;
        }
    };
    let tickets: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let gone = unregistered_tokens(messages, &tickets);
    if !gone.is_empty() {
        drop_push_tokens(&gone);
    }
}

fn unregistered_tokens(messages: &[Value], tickets: &Value) -> Vec<String> {
    let Some(list) = tickets["data"].as_array() else {
        return Vec::new();
    };
    list.iter()
        .zip(messages)
        .filter(|(ticket, _)| ticket["details"]["error"] == "DeviceNotRegistered")
        .filter_map(|(_, msg)| msg["to"].as_str().map(str::to_string))
        .collect()
}

#[derive(Default)]
struct TermSub {
    hash: u64,
    text: bool,
    lines: Vec<reflow::TextLine>,
    cursor: Option<[u32; 2]>,
}

#[derive(Default)]
struct Subs {
    device_id: String,
    terms: HashMap<String, TermSub>,
    chats: HashMap<String, ChatCursor>,
    live_seen: HashMap<String, Value>,
}

fn live_states() -> MutexGuard<'static, HashMap<String, Value>> {
    static LIVE: OnceLock<Mutex<HashMap<String, Value>>> = OnceLock::new();
    LIVE.get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

fn live_seq(live: &Value) -> u64 {
    live["seq"].as_u64().unwrap_or(0)
}

pub fn record_live(body: &[u8]) {
    let Ok(mut live) = serde_json::from_slice::<Value>(body) else {
        return;
    };
    let Some(tile_id) = live["tileId"].as_str().map(str::to_string) else {
        return;
    };
    if let Some(obj) = live.as_object_mut() {
        obj.remove("tileId");
    }
    let mut states = live_states();
    let newer = states.get(&tile_id).map_or(true, |prev| {
        live["turnId"] != prev["turnId"] || live_seq(&live) > live_seq(prev)
    });
    if newer {
        states.insert(tile_id, live);
    }
}

fn live_snapshot(tile_id: &str) -> Option<Value> {
    live_states().get(tile_id).cloned()
}
#[derive(Clone, Default, PartialEq)]
struct ChatCursor {
    session_id: String,
    offset: u64,
    status: Value,
    prompt: Value,
}

fn transcript_for(tile_id: &str) -> Result<(String, Option<PathBuf>), String> {
    let binding = read_binding_rec(tile_id).ok_or("tile has no agent")?;
    if binding["agent"].as_str() != Some("claude") {
        return Err("chat is only available for Claude tiles".into());
    }
    let session_id = binding["agentSessionId"].as_str().ok_or("agent session not started")?;
    Ok((session_id.to_string(), find_transcript(session_id, binding["cwd"].as_str())))
}

fn chat_update(tile_id: &str, cursor: Option<&ChatCursor>) -> Result<(ChatCursor, Value), String> {
    let (session_id, path) = transcript_for(tile_id)?;
    let offset = cursor.filter(|c| c.session_id == session_id).map(|c| c.offset);
    let chunk = match path {
        Some(path) => chat::read_chunk(&path, offset).map_err(|e| e.to_string())?,
        None => chat::ChatChunk {
            items: Vec::new(),
            offset: offset.unwrap_or(0),
        },
    };
    let next = ChatCursor {
        session_id: session_id.clone(),
        offset: chunk.offset,
        status: agent_status(tile_id),
        prompt: pending_prompt(tile_id),
    };
    let body = json!({
        "sessionId": session_id,
        "reset": offset.is_none(),
        "offset": chunk.offset,
        "items": chunk.items,
        "status": next.status,
        "prompt": next.prompt,
    });
    Ok((next, body))
}

fn chat_changed(tile_id: &str, cursor: &ChatCursor) -> bool {
    let Ok((session_id, path)) = transcript_for(tile_id) else {
        return false;
    };
    let grown = path
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(false, |m| m.len() > cursor.offset);
    session_id != cursor.session_id
        || grown
        || agent_status(tile_id) != cursor.status
        || pending_prompt(tile_id) != cursor.prompt
}

fn subscribe_chat(p: &Value, subs: &mut Subs) -> Result<Value, String> {
    let tile_id = str_param(p, "tileId")?;
    let resume = p["sessionId"].as_str().map(|session_id| ChatCursor {
        session_id: session_id.to_string(),
        offset: p["offset"].as_u64().unwrap_or(0),
        ..Default::default()
    });
    let (cursor, mut body) = chat_update(tile_id, resume.as_ref())?;
    subs.chats.insert(tile_id.to_string(), cursor);
    let live = live_snapshot(tile_id);
    let live = live.unwrap_or(Value::Null);
    subs.live_seen.insert(tile_id.to_string(), live.clone());
    body["live"] = live;
    Ok(body)
}

fn tile_cwd(tile_id: &str) -> Result<String, String> {
    let session = live_session(tile_id)?;
    let cwd = session.cwd.lock().unwrap_or_else(|e| e.into_inner()).clone();
    cwd.ok_or_else(|| "tile has no folder yet".to_string())
}

fn run_kind(p: &Value) -> &'static str {
    match p["kind"].as_str() {
        Some("build") => "build",
        _ => "run",
    }
}

fn parse_body(body: String) -> Result<Value, String> {
    let v: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    match v.get("error").and_then(Value::as_str) {
        Some(err) => Err(err.to_string()),
        None => Ok(v),
    }
}

async fn blocking<F>(f: F) -> Result<Value, String>
where
    F: FnOnce() -> Result<Value, String> + Send + 'static,
{
    tokio::task::spawn_blocking(f).await.map_err(|e| e.to_string())?
}

async fn run_overview(tile_id: &str) -> Result<Value, String> {
    let cwd = tile_cwd(tile_id)?;
    let detect_cwd = cwd.clone();
    let commands = blocking(move || Ok(run_commands::detect_json(&detect_cwd))).await?;
    let status = |kind: &str| parse_body(run_status_json(run_lookup_tile(kind, tile_id)));
    Ok(json!({ "cwd": cwd, "commands": commands, "run": status("run")?, "build": status("build")? }))
}

fn stop_run(tile_id: &str, kind: &str) {
    let Some(s) = run_lookup_tile(kind, tile_id) else { return };
    if s.exited.load(Ordering::Relaxed) {
        forget_session(&s);
    } else {
        run_stop(&s);
    }
}

async fn dispatch(method: &str, p: &Value, subs: &mut Subs) -> Result<Value, String> {
    match method {
        "tiles.list" => Ok(tiles_json()),
        "tile.open" => ui::open_tile(str_param(p, "cwd")?, p["agent"].as_str()),
        "run.list" => run_overview(str_param(p, "tileId")?).await,
        "run.start" => {
            let tile_id = str_param(p, "tileId")?;
            let cwd = tile_cwd(tile_id)?;
            parse_body(run_start_json(run_kind(p), tile_id, &cwd, str_param(p, "cmd")?).await)
        }
        "run.stop" => {
            stop_run(str_param(p, "tileId")?, run_kind(p));
            Ok(json!({}))
        }
        "git.status" => {
            let cwd = tile_cwd(str_param(p, "tileId")?)?;
            blocking(move || git_glance::status_json(&cwd)).await
        }
        "git.diff" => {
            let cwd = tile_cwd(str_param(p, "tileId")?)?;
            let path = str_param(p, "path")?.to_string();
            blocking(move || git_glance::diff_json(&cwd, &path)).await
        }
        "fs.list" => {
            let cwd = tile_cwd(str_param(p, "tileId")?)?;
            let path = p["path"].as_str().unwrap_or_default().to_string();
            blocking(move || files::list_json(&cwd, &path)).await
        }
        "fs.read" => {
            let cwd = tile_cwd(str_param(p, "tileId")?)?;
            let path = str_param(p, "path")?.to_string();
            blocking(move || files::read_json(&cwd, &path)).await
        }
        "term.subscribe" => {
            let tile_id = str_param(p, "tileId")?;
            live_session(tile_id)?;
            let text = p["view"].as_str() == Some("text");
            subs.terms.insert(tile_id.to_string(), TermSub { text, ..Default::default() });
            Ok(json!({}))
        }
        "term.unsubscribe" => {
            subs.terms.remove(str_param(p, "tileId")?);
            Ok(json!({}))
        }
        "chat.subscribe" => subscribe_chat(p, subs),
        "chat.unsubscribe" => {
            let tile_id = str_param(p, "tileId")?;
            subs.chats.remove(tile_id);
            subs.live_seen.remove(tile_id);
            Ok(json!({}))
        }
        "term.input" => {
            let session = live_session(str_param(p, "tileId")?)?;
            if session.exited.load(Ordering::Relaxed) {
                return Err("tile has exited".into());
            }
            write_input(&session, str_param(p, "data")?);
            Ok(json!({}))
        }
        "agent.attach" => {
            let data = str_param(p, "data")?.to_string();
            let mime = str_param(p, "mime")?.to_string();
            blocking(move || attach::save(&data, &mime)).await
        }
        "agent.send" => {
            let images: Vec<String> = p["images"]
                .as_array()
                .map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            attach::send(str_param(p, "tileId")?, str_param(p, "text")?, &images).await?;
            Ok(json!({}))
        }
        "push.register" => {
            let token = str_param(p, "token")?;
            if !is_push_token(token) {
                return Err("invalid push token".into());
            }
            register_push(&subs.device_id, token);
            Ok(json!({}))
        }
        _ => Err(format!("unknown method {method}")),
    }
}

async fn reply_for(req: &Value, subs: &mut Subs) -> Value {
    let method = req["m"].as_str().unwrap_or_default();
    match dispatch(method, &req["p"], subs).await {
        Ok(r) => json!({ "t": "res", "id": req["id"], "ok": true, "r": r }),
        Err(e) => json!({ "t": "res", "id": req["id"], "ok": false, "err": e }),
    }
}

fn frame_hash(frame: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    frame.hash(&mut h);
    h.finish()
}

async fn push_chats(tx: &mut Tx, chan: &mut Channel, subs: &mut Subs) -> Result<(), ()> {
    for (tile_id, cursor) in subs.chats.iter_mut() {
        if !chat_changed(tile_id, cursor) {
            continue;
        }
        let Ok((next, body)) = chat_update(tile_id, Some(cursor)) else {
            continue;
        };
        let quiet = next.session_id == cursor.session_id
            && next.status == cursor.status
            && next.prompt == cursor.prompt
            && body["items"] == json!([]);
        *cursor = next;
        if quiet {
            continue;
        }
        send_json(tx, chan, &json!({ "t": "evt", "e": "chat", "tileId": tile_id, "r": body })).await?;
    }
    for (tile_id, seen) in subs.live_seen.iter_mut() {
        let Some(live) = live_snapshot(tile_id) else {
            continue;
        };
        if live == *seen {
            continue;
        }
        *seen = live.clone();
        send_json(tx, chan, &json!({ "t": "evt", "e": "live", "tileId": tile_id, "r": live })).await?;
    }
    Ok(())
}

fn text_view(session: &Session) -> Option<reflow::TextView> {
    let mut parser = session.parser.lock().unwrap_or_else(|e| e.into_inner());
    if parser.screen().alternate_screen() {
        return None;
    }
    Some(reflow::logical_lines(parser.screen_mut(), TEXT_ROWS))
}

async fn push_terms(tx: &mut Tx, chan: &mut Channel, subs: &mut Subs) -> Result<(), ()> {
    for (tile_id, sub) in subs.terms.iter_mut() {
        let Some(session) = lookup(tile_id) else {
            continue;
        };
        let frame = build_frame(&session);
        let hash = frame_hash(&frame);
        if hash == sub.hash {
            continue;
        }
        sub.hash = hash;
        let Some(view) = sub.text.then(|| text_view(&session)).flatten() else {
            sub.lines.clear();
            send_frame(tx, chan, tile_id, &frame).await?;
            continue;
        };
        let patch = reflow::patch(&sub.lines, &view.lines);
        let moved = view.cursor != sub.cursor;
        sub.lines = view.lines;
        sub.cursor = view.cursor;
        if patch.is_none() && !moved {
            continue;
        }
        let mut r = patch.unwrap_or_else(|| json!({ "drop": 0, "keep": sub.lines.len(), "lines": [] }));
        r["cursor"] = json!(view.cursor);
        send_json(tx, chan, &json!({ "t": "evt", "e": "text", "tileId": tile_id, "r": r })).await?;
    }
    Ok(())
}

async fn serve(mut tx: Tx, mut rx: Rx, mut chan: Channel, device_id: String) {
    let mut subs = Subs {
        device_id,
        ..Default::default()
    };
    let mut tick = tokio::time::interval(FRAME_INTERVAL);
    loop {
        tokio::select! {
            _ = tick.tick() => {
                if !device_active(&subs.device_id)
                    || push_terms(&mut tx, &mut chan, &mut subs).await.is_err()
                    || push_chats(&mut tx, &mut chan, &mut subs).await.is_err()
                {
                    break;
                }
            }
            msg = rx.next() => match msg {
                Some(Ok(Message::Binary(sealed))) => {
                    let Some(plain) = chan.open(&sealed) else {
                        break;
                    };
                    let Some(req) = parse_json(&plain) else {
                        continue;
                    };
                    let reply = reply_for(&req, &mut subs).await;
                    if send_json(&mut tx, &mut chan, &reply).await.is_err() {
                        break;
                    }
                }
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
                _ => break,
            }
        }
    }
    let _ = tx.close().await;
}

async fn handle_conn(stream: TcpStream) {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE);
    config.max_frame_size = Some(MAX_MESSAGE);
    let Ok(ws) = tokio_tungstenite::accept_async_with_config(stream, Some(config)).await else {
        return;
    };
    let (tx, rx) = ws.split();
    serve_session(Box::pin(tx), Box::pin(rx)).await;
}

async fn serve_session(mut tx: Tx, mut rx: Rx) {
    let Ok(Some((chan, device_id))) = tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake(&mut tx, &mut rx)).await else {
        let _ = tx.close().await;
        return;
    };
    serve(tx, rx, chan, device_id).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_round_trips_and_rejects_replay() {
        let key = [7u8; 32];
        let mut server = Channel::new(&key);
        let client = XSalsa20Poly1305::new((&key).into());
        let sealed = client.encrypt(&nonce(DIR_CLIENT, 0), b"\x00{}".as_slice()).unwrap();
        assert_eq!(server.open(&sealed).as_deref(), Some(b"\x00{}".as_slice()));
        assert!(server.open(&sealed).is_none());
    }

    #[test]
    fn session_key_matches_both_sides() {
        let server = StaticSecret::from([1u8; 32]);
        let client = StaticSecret::from([2u8; 32]);
        let client_pk = PublicKey::from(&client).to_bytes();
        let (cn, sn) = ([3u8; 24], [4u8; 24]);
        let key = session_key(&server, &client_pk, &cn, &sn).unwrap();
        let shared = client.diffie_hellman(&PublicKey::from(&server));
        let mut h = Sha512::new();
        h.update(shared.as_bytes());
        h.update(client_pk);
        h.update(PublicKey::from(&server).as_bytes());
        h.update(cn);
        h.update(sn);
        assert_eq!(key, h.finalize()[..32]);
    }

    #[test]
    fn rejects_low_order_client_key() {
        assert!(session_key(&StaticSecret::from([1u8; 32]), &[0u8; 32], &[0; 24], &[0; 24]).is_none());
    }

    #[test]
    fn live_keeps_newest_seq_within_a_turn() {
        record_live(br#"{"tileId":"live-test","turnId":"a","seq":5,"text":"new"}"#);
        record_live(br#"{"tileId":"live-test","turnId":"a","seq":3,"text":"stale"}"#);
        assert_eq!(live_snapshot("live-test").unwrap()["text"], "new");
        record_live(br#"{"tileId":"live-test","turnId":"b","seq":1,"text":"next turn"}"#);
        assert_eq!(live_snapshot("live-test").unwrap()["text"], "next turn");
        assert!(live_snapshot("live-test").unwrap().get("tileId").is_none());
    }

    #[test]
    fn push_token_shape_is_checked() {
        assert!(is_push_token("ExponentPushToken[abc123]"));
        assert!(is_push_token("ExpoPushToken[abc123]"));
        assert!(!is_push_token("https://evil.example/ExponentPushToken[x]"));
        assert!(!is_push_token("ExponentPushToken[abc"));
    }

    #[test]
    fn unregistered_tickets_map_back_to_tokens() {
        let messages = vec![json!({ "to": "ExpoPushToken[a]" }), json!({ "to": "ExpoPushToken[b]" })];
        let tickets = json!({ "data": [
            { "status": "ok", "id": "x" },
            { "status": "error", "details": { "error": "DeviceNotRegistered" } },
        ] });
        assert_eq!(unregistered_tokens(&messages, &tickets), vec!["ExpoPushToken[b]".to_string()]);
    }

    #[test]
    fn pairing_code_compare_is_exact() {
        assert!(codes_match("ABCD", "ABCD"));
        assert!(!codes_match("ABCD", "ABCE"));
        assert!(!codes_match("ABCD", "ABC"));
    }
}
