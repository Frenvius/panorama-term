use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::{SinkExt, StreamExt};
use panorama_lib::remote::{connect, pair, start_proxy, Identity};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use std::{
    collections::HashMap,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};
use x25519_dalek::{PublicKey, StaticSecret};
struct Sidecar {
    child: Child,
    dir: PathBuf,
}
impl Drop for Sidecar {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn http(port: u16, path: &str) -> Value {
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream
        .write_all(
            format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
                .as_bytes(),
        )
        .await
        .unwrap();
    let mut data = Vec::new();
    stream.read_to_end(&mut data).await.unwrap();
    let body = data.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    serde_json::from_slice(&data[body..]).unwrap()
}
fn loopback(uri: &str, port: u16) -> String {
    let mut url = url::Url::parse(uri).unwrap();
    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.query_pairs_mut().clear().extend_pairs(
        pairs
            .iter()
            .map(|(k, v)| (k.as_str(), if k == "h" { "" } else { v.as_str() })),
    );
    url.query_pairs_mut()
        .append_pair("h", &format!("127.0.0.1:{port}"));
    url.into()
}
type Socket = tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>;
#[derive(Default)]
struct State {
    hosts: HashMap<String, mpsc::Sender<Message>>,
    waiting: HashMap<String, oneshot::Sender<Socket>>,
    next: u64,
}
struct Relay {
    port: u16,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Relay {
    async fn start() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));
        let task = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let state = state.clone();
                tokio::spawn(async move {
                    let path = Arc::new(std::sync::Mutex::new(String::new()));
                    let captured = path.clone();
                    let Ok(ws) = tokio_tungstenite::accept_hdr_async(
                        stream,
                        move |req: &tokio_tungstenite::tungstenite::handshake::server::Request,
                              response| {
                            *captured.lock().unwrap() = req.uri().path().to_owned();
                            Ok(response)
                        },
                    )
                    .await
                    else {
                        return;
                    };
                    let path = path.lock().unwrap().clone();
                    relay_socket(ws, path, state).await;
                });
            }
        });
        Self { port, task }
    }
    fn url(&self) -> String {
        format!("ws://127.0.0.1:{}", self.port)
    }
}
async fn close(ws: &mut Socket, code: u16) {
    let _ = ws
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::from(code),
            reason: "".into(),
        })))
        .await;
}
async fn relay_socket(mut ws: Socket, path: String, state: Arc<Mutex<State>>) {
    if path == "/v1/host" {
        let key = PublicKey::from(&StaticSecret::from([7; 32]));
        let mut nonce = [0; 32];
        getrandom::getrandom(&mut nonce).unwrap();
        if ws
            .send(Message::Text(
                json!({"pk":STANDARD.encode(key.as_bytes()),"n":STANDARD.encode(nonce)})
                    .to_string()
                    .into(),
            ))
            .await
            .is_err()
        {
            return;
        }
        let Some(Ok(Message::Text(register))) = ws.next().await else {
            return;
        };
        let Ok(register) = serde_json::from_str::<Value>(&register) else {
            return;
        };
        let Ok(pk) = STANDARD.decode(register["pk"].as_str().unwrap_or("")) else {
            return;
        };
        let hash = Sha512::new()
            .chain_update(b"panorama-relay/v1/room")
            .chain_update(pk)
            .finalize();
        let room: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
        if ws
            .send(Message::Text(
                json!({"t":"ready","room":room}).to_string().into(),
            ))
            .await
            .is_err()
        {
            return;
        }
        let (tx, mut rx) = mpsc::channel(32);
        state.lock().await.hosts.insert(room.clone(), tx);
        loop {
            tokio::select! {
                msg = ws.next() => match msg { Some(Ok(Message::Ping(_))) => { if ws.flush().await.is_err() { break; } }, Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break, _ => {} },
                Some(msg) = rx.recv() => if ws.send(msg).await.is_err() { break; },
            }
        }
        state.lock().await.hosts.remove(&room);
    } else if let Some(room) = path.strip_prefix("/v1/connect/") {
        let (host, id, receiver) = {
            let mut st = state.lock().await;
            let Some(host) = st.hosts.get(room).cloned() else {
                drop(st);
                close(&mut ws, 4404).await;
                return;
            };
            st.next += 1;
            let id = st.next.to_string();
            let (tx, rx) = oneshot::channel();
            st.waiting.insert(format!("{room}/{id}"), tx);
            (host, id, rx)
        };
        let _ = host
            .send(Message::Text(
                json!({"t":"open","id":id}).to_string().into(),
            ))
            .await;
        match tokio::time::timeout(Duration::from_secs(10), receiver).await {
            Ok(Ok(other)) => pipe(ws, other).await,
            _ => {
                state.lock().await.waiting.remove(&format!("{room}/{id}"));
                close(&mut ws, 4408).await;
            }
        }
    } else if let Some(key) = path.strip_prefix("/v1/accept/") {
        if let Some(tx) = state.lock().await.waiting.remove(key) {
            let _ = tx.send(ws);
        }
    }
}
async fn pipe(mut a: Socket, mut b: Socket) {
    loop {
        tokio::select! {
            msg = a.next() => match msg { Some(Ok(m @ (Message::Text(_) | Message::Binary(_)))) => if b.send(m).await.is_err() { break }, Some(Ok(Message::Ping(_))) => if a.flush().await.is_err() { break }, Some(Ok(Message::Pong(_))) => {}, _ => break },
            msg = b.next() => match msg { Some(Ok(m @ (Message::Text(_) | Message::Binary(_)))) => if a.send(m).await.is_err() { break }, Some(Ok(Message::Ping(_))) => if b.flush().await.is_err() { break }, Some(Ok(Message::Pong(_))) => {}, _ => break },
        }
    }
}
static ENV_LOCK: Mutex<()> = Mutex::const_new(());
fn unreachable(identity: Identity) -> Identity {
    let mut value = serde_json::to_value(identity).unwrap();
    value["addresses"] = json!([format!("127.0.0.1:{}", free_port())]);
    serde_json::from_value(value).unwrap()
}
#[tokio::test]
async fn relay_fallback_survives_idle() {
    let _guard = ENV_LOCK.lock().await;
    let relay = Relay::start().await;
    std::env::set_var("PANORAMA_RELAY_URL", relay.url());
    std::env::set_var("PANORAMA_REMOTE_HEARTBEAT_MS", "300");
    std::env::set_var("PANORAMA_REMOTE_IDLE_MS", "1500");
    let dir = std::env::temp_dir().join(format!("panorama-relay-e2e-{}", std::process::id()));
    let host_dir = dir.join("host");
    std::fs::create_dir_all(&host_dir).unwrap();
    std::env::set_var("PANORAMA_CONFIG_DIR", dir.join("client"));
    let port = free_port();
    let mobile_port = loop {
        let p = free_port();
        if p != port {
            break p;
        }
    };
    let rustc = Command::new("rustc").arg("-vV").output().unwrap();
    let info = String::from_utf8(rustc.stdout).unwrap();
    let triple = info.lines().find_map(|l| l.strip_prefix("host: ")).unwrap();
    let bin = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("sidecar-{triple}{}", std::env::consts::EXE_SUFFIX));
    let child = Command::new(bin)
        .env("HOME", &dir)
        .env("USERPROFILE", &dir)
        .env("PANORAMA_CONFIG_DIR", &host_dir)
        .env("PANORAMA_SIDECAR_PORT", port.to_string())
        .env("PANORAMA_MOBILE_PORT", mobile_port.to_string())
        .env("PANORAMA_RELAY_URL", relay.url())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _sidecar = Sidecar { child, dir };
    let secret_path = host_dir.join("host-service-secret");
    tokio::time::timeout(Duration::from_secs(20), async {
        while !secret_path.exists() {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let secret = std::fs::read_to_string(secret_path).unwrap();
    let mut request = format!("ws://127.0.0.1:{port}/host-service")
        .into_client_request()
        .unwrap();
    request
        .headers_mut()
        .insert("authorization", format!("Bearer {secret}").parse().unwrap());
    let (provider, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let provider_task = tokio::spawn(async move {
        let (mut tx, mut rx) = provider.split();
        while let Some(Ok(Message::Text(text))) = rx.next().await {
            let req: Value = serde_json::from_str(&text).unwrap();
            let _ = tx
                .send(Message::Text(
                    json!({"id":req["id"],"result":{"from":"provider"}})
                        .to_string()
                        .into(),
                ))
                .await;
        }
    });
    let pairing = http(port, "/mobile/pair?desktop=1").await;
    let identity = unreachable(
        pair(
            &loopback(pairing["uri"].as_str().unwrap(), mobile_port),
            "Desktop",
        )
        .await
        .unwrap(),
    );
    http(port, "/mobile/remote?on=1").await;
    tokio::time::timeout(Duration::from_secs(15), async {
        while http(port, "/mobile/state").await["relay"] != "online" {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let mut session = connect(&identity).await.unwrap();
    assert_eq!(
        session
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    let proxy = start_proxy(&identity).await.unwrap();
    assert!(proxy.relay());
    assert_eq!(
        proxy
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    let mut events = proxy.subscribe();
    tokio::time::sleep(Duration::from_secs(3)).await;
    while let Ok(event) = events.try_recv() {
        assert_ne!(event["event"], "reconnecting");
    }
    assert!(proxy.connected());
    assert_eq!(
        proxy
            .call("store_read", json!({"name":"workspace"}))
            .await
            .unwrap(),
        json!({"from":"provider"})
    );
    provider_task.abort();
}
#[tokio::test]
async fn relay_reports_offline_host() {
    let _guard = ENV_LOCK.lock().await;
    let relay = Relay::start().await;
    std::env::set_var("PANORAMA_RELAY_URL", relay.url());
    let mut host_key = [0u8; 32];
    getrandom::getrandom(&mut host_key).unwrap();
    let identity: Identity = serde_json::from_value(json!({"secret":vec![1u8;32],"host_key":host_key,"addresses":[format!("127.0.0.1:{}", free_port())],"device_id":"missing"})).unwrap();
    let error = match connect(&identity).await {
        Err(e) => e,
        Ok(_) => panic!("unexpected connection"),
    };
    assert_eq!(error, "Host is offline (relay)");
}
