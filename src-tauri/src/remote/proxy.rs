use super::{client, desktop_routes, tunnel, Identity};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, oneshot},
};
use tokio_tungstenite::tungstenite::Message;

async fn before_deadline<T>(
    deadline: tokio::time::Instant,
    future: impl std::future::Future<Output = T>,
) -> Result<T, tokio::time::error::Elapsed> {
    tokio::time::timeout_at(deadline, future).await
}
fn reset_idle(deadline: std::pin::Pin<&mut tokio::time::Sleep>, timeout: std::time::Duration) {
    deadline.reset(tokio::time::Instant::now() + timeout);
}

fn timing() -> (std::time::Duration, std::time::Duration) {
    static TIMING: std::sync::OnceLock<(std::time::Duration, std::time::Duration)> = std::sync::OnceLock::new();
    *TIMING.get_or_init(|| {
        let value = |name, default| std::time::Duration::from_millis(std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok()).filter(|v| *v > 0).unwrap_or(default));
        (value("PANORAMA_REMOTE_HEARTBEAT_MS", 15000), value("PANORAMA_REMOTE_IDLE_MS", 45000))
    })
}

struct StreamProgress {
    started: tokio::time::Instant,
    last: Option<tokio::time::Instant>,
    up: usize,
    down: usize,
}
fn stream_progress(streams: &mut HashMap<u64, StreamProgress>, plain: &[u8], up: bool, events: &broadcast::Sender<Value>) {
    if plain.first() == Some(&2) && plain.len() >= 9 {
        let id = u64::from_le_bytes(plain[1..9].try_into().unwrap());
        if let Some(stream) = streams.get_mut(&id) {
            let done = if up { stream.up += plain.len() - 9; stream.up } else { stream.down += plain.len() - 9; stream.down };
            let now = tokio::time::Instant::now();
            if now.duration_since(stream.started) >= std::time::Duration::from_secs(2)
                && stream.last.is_none_or(|last| now.duration_since(last) >= std::time::Duration::from_millis(250)) {
                stream.last = Some(now);
                let _ = events.send(json!({"event":"remote-transfer", "payload":{"direction":if up {"up"} else {"down"},"done":done,"total":null}}));
            }
        }
    } else if plain.first() == Some(&0) {
        if let Ok(v) = serde_json::from_slice::<Value>(&plain[1..]) {
            if v["t"] == "stream" && (v["op"] == "close" || v["op"] == "cancel") {
                if let Some(stream) = v["id"].as_u64().and_then(|id| streams.remove(&id)) {
                    if stream.last.is_some() { let _ = events.send(json!({"event":"remote-transfer","payload":{"done":true}})); }
                }
            }
        }
    }
}

struct Open {
    id: u64,
    method: String,
    target: String,
    ws: bool,
    pipe: tokio::io::DuplexStream,
}
type Reply = oneshot::Sender<Result<Value, String>>;
struct Call {
    command: String,
    args: Value,
    reply: Reply,
}
pub struct Proxy {
    pub endpoint: String,
    task: tokio::task::JoinHandle<()>,
    accept_task: tokio::task::AbortHandle,
    calls: mpsc::Sender<Call>,
    events: broadcast::Sender<Value>,
    connected: Arc<AtomicBool>,
    relay: Arc<AtomicBool>,
    error: Arc<std::sync::Mutex<Option<String>>>,
}
impl Proxy {
    pub fn subscribe(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }
    pub fn relay(&self) -> bool {
        self.relay.load(Ordering::Acquire)
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap().clone()
    }
    pub fn connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }
    pub async fn call(&self, command: &str, args: Value) -> Result<Value, String> {
        self.caller().call(command, args).await
    }
    pub fn caller(&self) -> Caller {
        Caller(self.calls.clone())
    }
}
#[derive(Clone)]
pub struct Caller(mpsc::Sender<Call>);
impl Caller {
    pub async fn call(&self, command: &str, args: Value) -> Result<Value, String> {
        let (reply, rx) = oneshot::channel();
        self.0
            .send(Call {
                command: command.into(),
                args,
                reply,
            })
            .await
            .map_err(|_| "Connection closed")?;
        tokio::time::timeout(std::time::Duration::from_secs(65), rx)
            .await
            .map_err(|_| "Host call timed out")?
            .map_err(|_| "Connection closed")?
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
        self.accept_task.abort();
    }
}

fn parse_request(
    head: &str,
    token: &str,
    port: u16,
) -> Option<(String, String, bool, String, Option<String>)> {
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?;
    let path = first.next()?;
    if first.next()? != "HTTP/1.1" || first.next().is_some() {
        return None;
    }
    let prefix = format!("/{token}");
    let target = path.strip_prefix(&prefix)?;
    if !target.starts_with('/') {
        return None;
    }
    let mut headers = HashMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        if headers
            .insert(name.to_ascii_lowercase(), value.trim())
            .is_some()
        {
            return None;
        }
    }
    if headers.get("host").copied() != Some(format!("127.0.0.1:{port}").as_str()) {
        return None;
    }
    if headers
        .get("origin")
        .is_some_and(|o| !super::local_origins::LOCAL_ORIGINS.contains(o))
    {
        return None;
    }
    let ws = headers
        .get("upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    if !desktop_routes::allowed(method, target, ws) {
        return None;
    }
    let tail = head.split_once("\r\n")?.1;
    Some((
        method.to_owned(),
        target.to_owned(),
        ws,
        format!("{method} {target} HTTP/1.1\r\n{tail}"),
        headers.get("origin").map(|v| v.to_string()),
    ))
}
pub async fn start_proxy(identity: &Identity) -> Result<Proxy, String> {
    start(identity, false).await
}
pub async fn start_offline(identity: &Identity) -> Result<Proxy, String> {
    start(identity, true).await
}
async fn start(identity: &Identity, offline: bool) -> Result<Proxy, String> {
    let first = if offline {
        None
    } else {
        Some(client::handshake(identity, json!({"t":"auth"})).await?)
    };
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let mut bytes = [0u8; 24];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let endpoint = format!("127.0.0.1:{port}/{token}");
    let (open_tx, mut open_rx) = mpsc::channel::<Open>(64);
    let (calls, mut call_rx) = mpsc::channel::<Call>(64);
    let (events, _) = broadcast::channel::<Value>(256);
    let event_tx = events.clone();
    let connected = Arc::new(AtomicBool::new(first.is_some()));
    let connection_status = connected.clone();
    let relay = Arc::new(AtomicBool::new(
        first
            .as_ref()
            .is_some_and(|c| matches!(c.3, client::Transport::Relay)),
    ));
    let relay_status = relay.clone();
    let error = Arc::new(std::sync::Mutex::new(None));
    let error_status = error.clone();
    let accept_task = tokio::spawn(async move {
        let ids = Arc::new(AtomicU64::new(1));
        while let Ok((socket, _)) = listener.accept().await {
            let _ = socket.set_nodelay(true);
            let tx = open_tx.clone();
            let token = token.clone();
            let id = ids.fetch_add(1, Ordering::Relaxed);
            tokio::spawn(async move {
                handle(socket, tx, &token, port, id).await;
            });
        }
    });
    let accept_abort = accept_task.abort_handle();
    let mut identity = identity.clone();
    let task = tokio::spawn(async move {
        let mut next = first;
        let mut delay = 100u64;
        let mut id = 0u64;
        loop {
            let (mut ws, mut channel, welcome, address) = match next.take() {
                Some(connection) => connection,
                None => match client::handshake(&identity, json!({"t":"auth"})).await {
                    Ok(connection) => connection,
                    Err(error) => {
                        *error_status.lock().unwrap() = Some(error.clone());
                        let _ = event_tx.send(json!({"event":"reconnecting", "error":error}));
                        tokio::select! {
                            _ = tokio::time::sleep(std::time::Duration::from_millis(delay)) => {},
                            Some(call) = call_rx.recv() => { let _ = call.reply.send(Err("Remote host reconnecting".into())); },
                            Some(_) = open_rx.recv() => {},
                        }
                        delay = (delay * 2).min(5000);
                        continue;
                    }
                },
            };
            if let client::Transport::Lan(address) = &address {
                identity.update_addresses(&welcome, address);
            }
            let _ = identity.save();
            let mut subscribe = vec![0];
            subscribe
                .extend_from_slice(json!({"m":"host.subscribe","id":0}).to_string().as_bytes());
            if tokio::time::timeout(
                std::time::Duration::from_secs(45),
                ws.send(Message::Binary(channel.seal(&subscribe).into())),
            )
            .await
            .map_or(true, |r| r.is_err())
            {
                connection_status.store(false, Ordering::Release);
                let _ = event_tx.send(json!({"event":"reconnecting"}));
                continue;
            }
            delay = 100;
            relay_status.store(
                matches!(address, client::Transport::Relay),
                Ordering::Release,
            );
            *error_status.lock().unwrap() = None;
            connection_status.store(true, Ordering::Release);
            let _ = event_tx.send(json!({"event":"connected", "transport": if matches!(address, client::Transport::Relay) { "relay" } else { "lan" }}));
            let _ = event_tx.send(json!({"event":"resync"}));
            let (out, mut incoming) = mpsc::channel::<Vec<u8>>(64);
            let mut mux = tunnel::Mux::new(out);
            let mut pending: HashMap<u64, Reply> = HashMap::new();
            let mut transfers: HashMap<u64, StreamProgress> = HashMap::new();
            let mut pacer = matches!(address, client::Transport::Relay).then(|| super::pacer::Pacer::new(640_000, 256 * 1024));
            let (heartbeat, idle) = timing();
            let mut ping = tokio::time::interval(heartbeat);
            ping.tick().await;
            let deadline = tokio::time::sleep(idle);
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    _ = &mut deadline => break,
                    _ = ping.tick() => {
                        if before_deadline(deadline.deadline(), ws.send(Message::Ping(Vec::new().into()))).await.map_or(true, |r| r.is_err()) { break; }
                        let mut plain = vec![0]; plain.extend_from_slice(br#"{"t":"heartbeat"}"#);
                        if before_deadline(deadline.deadline(), ws.send(Message::Binary(channel.seal(&plain).into()))).await.map_or(true, |r| r.is_err()) { break; }
                    },
                    Some(open) = open_rx.recv() => {
                        let request = json!({"t":"stream","op":"open","id":open.id,"method":open.method,"target":open.target,"ws":open.ws});
                        let mut plain = vec![0]; plain.extend_from_slice(request.to_string().as_bytes());
                        if !mux.insert(open.id, open.pipe) { continue; }
                        if pacer.is_some() && !open.ws { transfers.insert(open.id, StreamProgress { started: tokio::time::Instant::now(), last: None, up: 0, down: 0 }); }
                        if plain.len() > 16 * 1024 { if let Some(pacer) = &mut pacer { pacer.take(plain.len()).await; } }
                        if before_deadline(deadline.deadline(), ws.send(Message::Binary(channel.seal(&plain).into()))).await.map_or(true, |r| r.is_err()) { break; }
                    }
                    Some(call) = call_rx.recv() => {
                        id += 1;
                        let request = json!({"m":"host.call","p":{"command":call.command,"args":call.args},"id":id});
                        let mut plain = vec![0]; plain.extend_from_slice(request.to_string().as_bytes());
                        pending.insert(id, call.reply);
                        if plain.len() > 16 * 1024 { if let Some(pacer) = &mut pacer { pacer.take(plain.len()).await; } }
                        if before_deadline(deadline.deadline(), ws.send(Message::Binary(channel.seal(&plain).into()))).await.map_or(true, |r| r.is_err()) { break; }
                    }
                    Some(plain) = incoming.recv() => {
                        mux.sent(&plain);
                        stream_progress(&mut transfers, &plain, true, &event_tx);
                        if plain.len() > 16 * 1024 { if let Some(pacer) = &mut pacer { pacer.take(plain.len()).await; } }
                        if before_deadline(deadline.deadline(), ws.send(Message::Binary(channel.seal(&plain).into()))).await.map_or(true, |r| r.is_err()) { break; }
                    }
                    message = ws.next() => {
                        if message.as_ref().is_some_and(|m| m.is_ok()) { reset_idle(deadline.as_mut(), idle); }
                        match message {
                        Some(Ok(Message::Binary(data))) => {
                            let Some(plain) = channel.open(&data) else { break };
                            stream_progress(&mut transfers, &plain, false, &event_tx);
                            if plain.first() == Some(&0) {
                                if let Ok(v) = serde_json::from_slice::<Value>(&plain[1..]) {
                                    if v["t"] == "heartbeat" { continue; }
                                    if v["t"] == "res" {
                                        if let Some(reply) = v["id"].as_u64().and_then(|id| pending.remove(&id)) {
                                            let result = if v["ok"] == true {
                                                Ok(v["r"].clone())
                                            } else {
                                                Err(v["err"].as_str().unwrap_or("Host call failed").into())
                                            };
                                            let _ = reply.send(result);
                                        }
                                        continue;
                                    }
                                    if v["t"] == "host.event" { let _ = event_tx.send(v); continue; }
                                }
                            }
                            if !mux.handle(&plain) { break; }
                        },
                        Some(Ok(Message::Ping(_))) => if before_deadline(deadline.deadline(), ws.flush()).await.map_or(true, |r| r.is_err()) { break; },
                        Some(Ok(Message::Pong(_))) => {}, _ => break,
                    }}
                }
            }
            if transfers.values().any(|t| t.last.is_some()) { let _ = event_tx.send(json!({"event":"remote-transfer", "payload":{"done":true}})); }
            for (_, reply) in pending {
                let _ = reply.send(Err("Connection lost; result unknown".into()));
            }
            connection_status.store(false, Ordering::Release);
            let _ = event_tx.send(json!({"event":"reconnecting"}));
        }
    });
    Ok(Proxy {
        endpoint,
        task,
        accept_task: accept_abort,
        calls,
        events,
        connected,
        relay,
        error,
    })
}
async fn handle(mut socket: TcpStream, tx: mpsc::Sender<Open>, token: &str, port: u16, id: u64) {
    let mut head = Vec::new();
    let mut byte = [0];
    let read = async {
        while head.len() < 16384 && !head.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await?;
            head.push(byte[0]);
        }
        Ok::<_, std::io::Error>(())
    };
    if tokio::time::timeout(std::time::Duration::from_secs(10), read)
        .await
        .ok()
        .and_then(Result::ok)
        .is_none()
    {
        return;
    }
    let parsed = std::str::from_utf8(&head)
        .ok()
        .filter(|_| head.ends_with(b"\r\n\r\n"))
        .and_then(|h| parse_request(h, token, port));
    let Some((method, target, ws, rewritten, origin)) = parsed else {
        let _ = socket
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        return;
    };
    let (pipe, mut worker) = tokio::io::duplex(64 * 1024);
    if tx
        .send(Open {
            id,
            method,
            target,
            ws,
            pipe,
        })
        .await
        .is_err()
    {
        return;
    }
    if worker.write_all(rewritten.as_bytes()).await.is_err() {
        return;
    }
    if ws {
        let _ = tokio::io::copy_bidirectional(&mut socket, &mut worker).await;
        return;
    }
    let (mut sr, mut sw) = socket.into_split();
    let (mut wr, mut ww) = tokio::io::split(worker);
    let request = async {
        let _ = tokio::io::copy(&mut sr, &mut ww).await;
        let _ = ww.shutdown().await;
    };
    let response = async {
        let mut head = Vec::new();
        let mut byte = [0];
        while head.len() < 16384 && !head.ends_with(b"\r\n\r\n") {
            if wr.read_exact(&mut byte).await.is_err() {
                return;
            }
            head.push(byte[0]);
        }
        if !head.ends_with(b"\r\n\r\n") {
            return;
        }
        let Ok(text) = std::str::from_utf8(&head) else {
            return;
        };
        let mut filtered = String::new();
        for line in text.split("\r\n").filter(|l| !l.is_empty()) {
            if line
                .split_once(':')
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case("access-control-allow-origin"))
            {
                continue;
            }
            filtered.push_str(line);
            filtered.push_str("\r\n");
        }
        if let Some(origin) = origin {
            filtered.push_str(&format!(
                "Access-Control-Allow-Origin: {origin}\r\nVary: Origin\r\n"
            ));
        }
        filtered.push_str("\r\n");
        if sw.write_all(filtered.as_bytes()).await.is_ok() {
            let _ = tokio::io::copy(&mut wr, &mut sw).await;
        }
        let _ = sw.shutdown().await;
    };
    tokio::join!(request, response);
}
#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn blocked_send_obeys_deadline() {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(20);
        assert!(
            super::before_deadline(deadline, std::future::pending::<()>())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn idle_deadline_is_extended_by_incoming_traffic() {
        let timeout = std::time::Duration::from_millis(80);
        let deadline = tokio::time::sleep(timeout);
        tokio::pin!(deadline);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        super::reset_idle(deadline.as_mut(), timeout);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), deadline.as_mut())
                .await
                .is_err()
        );
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), deadline.as_mut())
                .await
                .is_ok()
        );
    }

    use super::*;
    #[test]
    fn request_policy() {
        let head = "GET /abc/pty?tileId=x HTTP/1.1\r\nHost: 127.0.0.1:123\r\nOrigin: tauri://localhost\r\nUpgrade: websocket\r\n\r\n";
        assert!(parse_request(head, "abc", 123).is_some());
        assert!(parse_request(head, "bad", 123).is_none());
        assert!(parse_request(
            &head.replace("tauri://localhost", "https://evil.test"),
            "abc",
            123
        )
        .is_none());
        assert!(
            parse_request(&head.replace("/pty?tileId=x", "/mobile/state"), "abc", 123).is_none()
        );
        assert!(parse_request(&head.replace("127.0.0.1:123", "evil.test"), "abc", 123).is_none());
    }
}
