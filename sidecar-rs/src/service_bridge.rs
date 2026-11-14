use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use subtle::ConstantTimeEq;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

struct Provider {
    generation: u64,
    tx: mpsc::Sender<Value>,
}
struct State {
    provider: Option<Provider>,
    pending: HashMap<u64, oneshot::Sender<Result<Value, String>>>,
}
static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static NEXT: AtomicU64 = AtomicU64::new(1);
static SECRET: OnceLock<String> = OnceLock::new();
static EVENTS: OnceLock<broadcast::Sender<Value>> = OnceLock::new();
fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            provider: None,
            pending: HashMap::new(),
        })
    })
}
fn events() -> &'static broadcast::Sender<Value> {
    EVENTS.get_or_init(|| broadcast::channel(256).0)
}

pub fn init() -> Result<(), String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| e.to_string())?;
    let secret: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let dir = crate::panorama_dir().ok_or("missing configuration directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("host-service-secret");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(0o600);
        // Also fix permissions if the file already exists.
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .or_else(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(e)
                }
            })
            .map_err(|e| e.to_string())?;
    }
    use std::io::Write;
    opts.open(path)
        .and_then(|mut f| f.write_all(secret.as_bytes()))
        .map_err(|e| e.to_string())?;
    SECRET
        .set(secret)
        .map_err(|_| "secret already initialized".to_string())
}

pub fn authorized(headers: &HashMap<String, String>) -> bool {
    authorized_with_secret(headers, SECRET.get().map(String::as_str))
}

fn authorized_with_secret(headers: &HashMap<String, String>, secret: Option<&str>) -> bool {
    headers.get("origin").is_none()
        && headers
            .get("host")
            .map(|h| h == &format!("127.0.0.1:{}", crate::port()))
            .unwrap_or(false)
        && headers
            .get("authorization")
            .and_then(|h| h.strip_prefix("Bearer "))
            .zip(secret)
            .map(|(a, b)| bool::from(a.as_bytes().ct_eq(b.as_bytes())))
            .unwrap_or(false)
}

fn replace(tx: mpsc::Sender<Value>) -> u64 {
    let generation = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut state = state().lock().unwrap();
    state.pending.clear();
    state.provider = Some(Provider { generation, tx });
    let _ = events().send(json!({"event":"resync"}));
    generation
}
fn disconnect(generation: u64) {
    let mut state = state().lock().unwrap();
    if state.provider.as_ref().map(|p| p.generation) == Some(generation) {
        state.provider = None;
        state.pending.clear();
        let _ = events().send(json!({"event":"resync"}));
    }
}

// Internal API for the desktop transport; it is not exposed as an HTTP route.
pub async fn call(command: &str, args: Value) -> Result<Value, String> {
    call_scoped(command, args, None).await
}

pub async fn call_scoped(
    command: &str,
    args: Value,
    session: Option<u64>,
) -> Result<Value, String> {
    call_with_session(command, args, session, std::time::Duration::from_secs(60)).await
}

#[cfg(test)]
async fn call_with_timeout(
    command: &str,
    args: Value,
    timeout: std::time::Duration,
) -> Result<Value, String> {
    call_with_session(command, args, None, timeout).await
}

async fn call_with_session(
    command: &str,
    args: Value,
    session: Option<u64>,
    timeout: std::time::Duration,
) -> Result<Value, String> {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (reply, rx) = oneshot::channel();
    let tx = {
        let mut state = state().lock().unwrap();
        let tx = state
            .provider
            .as_ref()
            .ok_or("Host app unavailable")?
            .tx
            .clone();
        state.pending.insert(id, reply);
        tx
    };
    let result = tokio::time::timeout(timeout, async {
        tx.send(json!({"id": id, "command": command, "args": args, "session": session}))
            .await
            .map_err(|_| "Host app unavailable".to_string())?;
        rx.await.map_err(|_| "Host app unavailable".to_string())?
    })
    .await;
    state().lock().unwrap().pending.remove(&id);
    result.map_err(|_| "Host service request timed out".to_string())?
}
pub fn subscribe() -> broadcast::Receiver<Value> {
    events().subscribe()
}

pub async fn serve(ws: WebSocketStream<TcpStream>) {
    let (mut sink, mut stream) = ws.split();
    let (tx, mut rx) = mpsc::channel::<Value>(64);
    let generation = replace(tx);
    loop {
        tokio::select! {
            outgoing = rx.recv() => match outgoing {
                Some(v) => if sink.send(Message::Text(v.to_string())).await.is_err() { break },
                None => {
                    use tokio_tungstenite::tungstenite::protocol::{CloseFrame, frame::coding::CloseCode};
                    let _ = sink.send(Message::Close(Some(CloseFrame { code: CloseCode::Normal, reason: "provider replaced".into() }))).await;
                    break;
                },
            },
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(s))) => if state().lock().unwrap().provider.as_ref()
                    .map(|p| p.generation) != Some(generation) { break }
                else if let Ok(v) = serde_json::from_str::<Value>(&s) {
                    if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        let reply = { let mut state = state().lock().unwrap();
                            if state.provider.as_ref().map(|p| p.generation) == Some(generation) { state.pending.remove(&id) } else { None }
                        };
                        if let Some(reply) = reply {
                            let result = if let Some(error) = v.get("error").and_then(Value::as_str) {
                                Err(error.to_string())
                            } else {
                                Ok(v.get("result").cloned().unwrap_or(Value::Null))
                            };
                            let _ = reply.send(result);
                        }
                    } else if v.get("event").and_then(Value::as_str).is_some() {
                        let _ = events().send(v);
                    }
                },
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {},
            }
        }
    }
    disconnect(generation);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authorization_rejects_wrong_secret_and_browser_origin() {
        let mut headers = HashMap::from([
            ("host".into(), format!("127.0.0.1:{}", crate::port())),
            ("authorization".into(), "Bearer wrong".into()),
        ]);
        assert!(!authorized_with_secret(&headers, Some("test-secret")));
        headers.insert("authorization".into(), "Bearer test-secret".into());
        assert!(authorized_with_secret(&headers, Some("test-secret")));
        headers.insert("origin".into(), "https://example.com".into());
        assert!(!authorized_with_secret(&headers, Some("test-secret")));
    }
    #[test]
    fn isolated_bridge_test() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "service_bridge::tests::unavailable_and_replacement",
                "--ignored",
            ])
            .status()
            .unwrap();
        assert!(status.success());
    }
    #[tokio::test]
    #[ignore]
    async fn unavailable_and_replacement() {
        assert_eq!(
            call("store_read", json!({})).await.unwrap_err(),
            "Host app unavailable"
        );
        let (tx, _rx) = mpsc::channel(1);
        let old = replace(tx);
        let (tx, _rx) = mpsc::channel(1);
        let new = replace(tx);
        disconnect(old);
        assert!(state().lock().unwrap().provider.is_some());
        disconnect(new);
        assert_eq!(
            call("store_read", json!({})).await.unwrap_err(),
            "Host app unavailable"
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            serve(tokio_tungstenite::accept_async(socket).await.unwrap()).await;
        });
        let (mut client, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
            .await
            .unwrap();
        while state().lock().unwrap().provider.is_none() {
            tokio::task::yield_now().await;
        }
        let mut subscription = subscribe();
        client
            .send(Message::Text(
                json!({"event":"note:changed","payload":{"content":"hello"}}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(subscription.recv().await.unwrap()["event"], "note:changed");
        let request = tokio::spawn(async { call("store_read", json!({"name":"test"})).await });
        let Message::Text(raw) = client.next().await.unwrap().unwrap() else {
            panic!("expected request")
        };
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(v["command"], "store_read");
        client
            .send(Message::Text(
                json!({"id":v["id"],"result":[0,255]}).to_string(),
            ))
            .await
            .unwrap();
        assert_eq!(request.await.unwrap().unwrap(), json!([0, 255]));
        client.close(None).await.unwrap();
        server.await.unwrap();
        assert_eq!(
            call("store_read", json!({})).await.unwrap_err(),
            "Host app unavailable"
        );
        let (tx, _rx) = mpsc::channel(1);
        tx.send(json!({})).await.unwrap();
        let generation = replace(tx);
        assert_eq!(
            call_with_timeout(
                "store_read",
                json!({}),
                std::time::Duration::from_millis(10)
            )
            .await
            .unwrap_err(),
            "Host service request timed out"
        );
        assert!(state().lock().unwrap().pending.is_empty());
        disconnect(generation);
    }
}
