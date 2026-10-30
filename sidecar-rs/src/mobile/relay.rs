use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::{SinkExt, Stream, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha512};
use tokio::task::AbortHandle;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};
use x25519_dalek::{PublicKey, StaticSecret};

use super::{b64, decode_fixed, serve_session, MAX_MESSAGE};

const DEFAULT_URL: &str = "wss://relay.usepanorama.dev";
const PROOF_TAG: &[u8] = b"panorama-relay/v1/proof";
const REGISTER_TIMEOUT: Duration = Duration::from_secs(15);
const PING_EVERY: Duration = Duration::from_secs(25);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const OFF: u8 = 0;
const CONNECTING: u8 = 1;
const ONLINE: u8 = 2;

static STATUS: AtomicU8 = AtomicU8::new(OFF);

fn base_url() -> String {
    std::env::var("PANORAMA_RELAY_URL").unwrap_or_else(|_| DEFAULT_URL.into())
}

pub(super) fn status() -> &'static str {
    match STATUS.load(Ordering::Relaxed) {
        ONLINE => "online",
        CONNECTING => "connecting",
        _ => "off",
    }
}

pub(super) fn spawn(secret: StaticSecret) -> AbortHandle {
    STATUS.store(CONNECTING, Ordering::Relaxed);
    tokio::spawn(run(secret)).abort_handle()
}

pub(super) fn stop(task: AbortHandle) {
    task.abort();
    STATUS.store(OFF, Ordering::Relaxed);
}

fn ws_config() -> WebSocketConfig {
    let mut config = WebSocketConfig::default();
    config.max_message_size = Some(MAX_MESSAGE);
    config.max_frame_size = Some(MAX_MESSAGE);
    config
}

async fn run(secret: StaticSecret) {
    let mut backoff = Duration::from_secs(1);
    loop {
        STATUS.store(CONNECTING, Ordering::Relaxed);
        let started = Instant::now();
        if let Err(e) = control(&secret).await {
            eprintln!("mobile relay: {e}");
        }
        STATUS.store(CONNECTING, Ordering::Relaxed);
        if started.elapsed() > MAX_BACKOFF {
            backoff = Duration::from_secs(1);
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

fn proof(secret: &StaticSecret, challenge: &Value) -> Option<[u8; 32]> {
    let relay_pk: [u8; 32] = decode_fixed(challenge["pk"].as_str()?)?;
    let nonce: [u8; 32] = decode_fixed(challenge["n"].as_str()?)?;
    let shared = secret.diffie_hellman(&PublicKey::from(relay_pk));
    if !shared.was_contributory() {
        return None;
    }
    let digest = Sha512::new()
        .chain_update(PROOF_TAG)
        .chain_update(shared.as_bytes())
        .chain_update(nonce)
        .finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest[..32]);
    Some(out)
}

async fn next_json<S>(rx: &mut S) -> Result<Value, String>
where
    S: Stream<Item = Result<Message, WsError>> + Unpin,
{
    loop {
        match rx.next().await {
            Some(Ok(Message::Text(text))) => return serde_json::from_str(&text).map_err(|e| e.to_string()),
            Some(Ok(Message::Close(frame))) => return Err(format!("closed: {frame:?}")),
            Some(Ok(_)) => continue,
            Some(Err(e)) => return Err(e.to_string()),
            None => return Err("closed".into()),
        }
    }
}

async fn control(secret: &StaticSecret) -> Result<(), String> {
    let base = base_url();
    let (ws, _) = tokio_tungstenite::connect_async(format!("{base}/v1/host"))
        .await
        .map_err(|e| format!("connect: {e}"))?;
    let (mut tx, mut rx) = ws.split();
    let challenge = tokio::time::timeout(REGISTER_TIMEOUT, next_json(&mut rx))
        .await
        .map_err(|_| "no challenge")??;
    let proof = proof(secret, &challenge).ok_or("bad challenge")?;
    let register = json!({
        "t": "register",
        "pk": b64().encode(PublicKey::from(secret).as_bytes()),
        "proof": b64().encode(proof),
    });
    tx.send(Message::Text(register.to_string())).await.map_err(|e| e.to_string())?;
    let ready = tokio::time::timeout(REGISTER_TIMEOUT, next_json(&mut rx))
        .await
        .map_err(|_| "no ready")??;
    let room = ready["room"].as_str().filter(|_| ready["t"] == "ready").ok_or("refused")?.to_string();
    STATUS.store(ONLINE, Ordering::Relaxed);

    let mut ping = tokio::time::interval(PING_EVERY);
    loop {
        tokio::select! {
            msg = rx.next() => match msg {
                Some(Ok(Message::Text(text))) => {
                    let open: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
                    if let (Some("open"), Some(id)) = (open["t"].as_str(), open["id"].as_str()) {
                        tokio::spawn(accept(format!("{base}/v1/accept/{room}/{id}")));
                    }
                }
                Some(Ok(Message::Close(frame))) => return Err(format!("closed: {frame:?}")),
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("closed".into()),
            },
            _ = ping.tick() => {
                tx.send(Message::Ping(Vec::new())).await.map_err(|e| e.to_string())?;
            }
        }
    }
}

async fn accept(url: String) {
    match tokio_tungstenite::connect_async_with_config(url, Some(ws_config()), true).await {
        Ok((ws, _)) => {
            let (tx, rx) = ws.split();
            serve_session(Box::pin(tx), Box::pin(rx)).await;
        }
        Err(e) => eprintln!("mobile relay accept: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_matches_the_relay_derivation() {
        let host = StaticSecret::from([9u8; 32]);
        let relay = StaticSecret::from([4u8; 32]);
        let nonce = [7u8; 32];
        let challenge = json!({
            "pk": b64().encode(PublicKey::from(&relay).as_bytes()),
            "n": b64().encode(nonce),
        });
        let shared = relay.diffie_hellman(&PublicKey::from(&host));
        let expected: [u8; 32] = Sha512::new()
            .chain_update(PROOF_TAG)
            .chain_update(shared.as_bytes())
            .chain_update(nonce)
            .finalize()[..32]
            .try_into()
            .unwrap();
        assert_eq!(proof(&host, &challenge), Some(expected));
    }
}
