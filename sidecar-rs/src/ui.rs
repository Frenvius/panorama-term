use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::{seed_agent_binding, sessions, write_input};

const AGENT_COMMANDS: &[(&str, &str)] = &[("claude", "claude"), ("codex", "codex")];
const SPAWN_TIMEOUT: Duration = Duration::from_secs(20);
const SHELL_SETTLE: Duration = Duration::from_millis(800);
const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn channel() -> &'static broadcast::Sender<String> {
    static UI: OnceLock<broadcast::Sender<String>> = OnceLock::new();
    UI.get_or_init(|| broadcast::channel(16).0)
}

#[cfg(test)]
pub(crate) fn subscribe_events() -> broadcast::Receiver<String> {
    channel().subscribe()
}

pub(crate) struct TileGroup<'a> {
    pub id: &'a str,
    pub title: &'a str,
}

pub(crate) fn broadcast_grouped_tile_open(tile_id: &str, cwd: &str, group: Option<TileGroup>) {
    let mut event = json!({ "t": "tile.open", "tileId": tile_id, "cwd": cwd, "orchWorker": true });
    if let Some(g) = group {
        if let Some(obj) = event.as_object_mut() {
            obj.insert("group".into(), json!({ "id": g.id, "title": g.title }));
        }
    }
    send_event(event);
}

pub(crate) fn broadcast_orch_changed(run_id: &str) {
    send_event(json!({ "t": "orch.changed", "runId": run_id }));
}

pub(crate) fn has_subscribers() -> bool {
    channel().receiver_count() > 0
}

pub(crate) fn broadcast_agents_mail(parties: &[String]) {
    send_event(json!({ "t": "agents.mail", "parties": parties }));
}

pub(crate) fn broadcast_agents_waits(party: &str) {
    static PENDING: OnceLock<std::sync::Mutex<std::collections::BTreeSet<String>>> = OnceLock::new();
    let pending = PENDING.get_or_init(Default::default);
    let first = {
        let mut set = pending.lock().unwrap_or_else(|e| e.into_inner());
        let first = set.is_empty();
        set.insert(party.to_string());
        first
    };
    if !first {
        return;
    }
    crate::agents::spawn_background(async move {
        tokio::time::sleep(Duration::from_millis(250)).await;
        let parties: Vec<String> = std::mem::take(&mut *pending.lock().unwrap_or_else(|e| e.into_inner())).into_iter().collect();
        send_event(json!({ "t": "agents.waits", "parties": parties }));
    });
}

pub(crate) fn broadcast_agents_routines() {
    static PENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if PENDING.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let sent_later = crate::agents::spawn_background_checked(async {
        tokio::time::sleep(Duration::from_millis(250)).await;
        PENDING.store(false, std::sync::atomic::Ordering::Release);
        send_event(json!({ "t": "agents.routines" }));
    });
    if !sent_later {
        PENDING.store(false, std::sync::atomic::Ordering::Release);
        send_event(json!({ "t": "agents.routines" }));
    }
}

pub(crate) fn broadcast_agents_rollover(agent_id: &str) {
    send_event(json!({ "t": "agents.rollover", "agentId": agent_id }));
}

pub(crate) fn broadcast_agents_memory(agent_id: &str) {
    send_event(json!({ "t": "agents.memory", "agentId": agent_id }));
}

pub(crate) fn broadcast_agents_changed(agent_id: &str) {
    send_event(json!({ "t": "agents.changed", "agentId": agent_id }));
}

pub(crate) fn broadcast_orch_notify(event: Value) {
    send_event(event);
}

pub(crate) fn broadcast_tile_wake(tile_id: &str) {
    send_event(json!({ "t": "tile.wake", "tileId": tile_id }));
}

pub(crate) fn broadcast_app_update() -> bool {
    if channel().receiver_count() == 0 {
        return false;
    }
    send_event(json!({ "t": "app.update" }));
    true
}

fn send_event(event: serde_json::Value) {
    if channel().receiver_count() > 0 {
        let _ = channel().send(event.to_string());
    }
}

pub async fn serve(ws: WebSocketStream<TcpStream>) {
    let mut events = channel().subscribe();
    let (mut tx, mut rx) = ws.split();
    let cap = json!({ "t": "ui.capability", "token": crate::agents::ui_user_cap() }).to_string();
    if tx.send(Message::Text(cap)).await.is_err() {
        return;
    }
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

pub(crate) fn new_tile_id() -> String {
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
    if let Some(name) = agent {
        seed_agent_binding(&tile_id, name, cwd);
    }
    let event = json!({ "t": "tile.open", "tileId": tile_id, "cwd": cwd });
    channel().send(event.to_string()).map_err(|_| "Panorama window is not open".to_string())?;
    if let Some(command) = command {
        launch_agent(tile_id.clone(), command);
    }
    Ok(json!({ "tileId": tile_id }))
}
