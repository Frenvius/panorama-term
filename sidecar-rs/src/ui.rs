use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::{sessions, write_input};

const AGENT_COMMANDS: &[(&str, &str)] = &[("claude", "claude")];
const SPAWN_TIMEOUT: Duration = Duration::from_secs(20);
const SHELL_SETTLE: Duration = Duration::from_millis(800);
const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn channel() -> &'static broadcast::Sender<String> {
    static UI: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    UI.get_or_init(|| broadcast::channel(16).0)
}

pub async fn serve(ws: WebSocketStream<TcpStream>) {
    let mut events = channel().subscribe();
    let (mut tx, mut rx) = ws.split();
    loop {
        tokio::select! {
            evt = events.recv() => match evt {
                Ok(text) => {
                    if tx.send(Message::Text(text)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            msg = rx.next() => match msg {
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                _ => {}
            }
        }
    }
    let _ = tx.close().await;
}

fn new_tile_id() -> String {
    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).expect("os rng");
    bytes.iter().map(|b| ID_ALPHABET[*b as usize % ID_ALPHABET.len()] as char).collect()
}

fn launch_agent(tile_id: String, command: &'static str) {
    tokio::spawn(async move {
        let deadline = Instant::now() + SPAWN_TIMEOUT;
        while Instant::now() < deadline {
            let session = sessions().lock().unwrap().get(&tile_id).cloned();
            if let Some(session) = session {
                tokio::time::sleep(SHELL_SETTLE).await;
                write_input(&session, &format!("{command}\r"));
                return;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
}

pub fn open_tile(cwd: &str, agent: Option<&str>) -> Result<Value, String> {
    if !Path::new(cwd).is_dir() {
        return Err("folder not found".into());
    }
    let command = match agent {
        None => None,
        Some(name) => Some(
            AGENT_COMMANDS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, cmd)| *cmd)
                .ok_or_else(|| format!("unknown agent {name}"))?,
        ),
    };
    if channel().receiver_count() == 0 {
        return Err("Panorama window is not open".into());
    }
    let tile_id = new_tile_id();
    let event = json!({ "t": "tile.open", "tileId": tile_id, "cwd": cwd });
    channel().send(event.to_string()).map_err(|_| "Panorama window is not open".to_string())?;
    if let Some(command) = command {
        launch_agent(tile_id.clone(), command);
    }
    Ok(json!({ "tileId": tile_id }))
}
