use std::collections::HashMap;
use std::time::Instant;

use base64::Engine as _;
use serde_json::{json, Value};

use super::jwt;
use super::{ApiError, Account, AccountRec, Service};

const B64_PREFIX: &str = "go-keyring-base64:";
const HEX_PREFIX: &str = "go-keyring-encoded:";

pub struct Decoded {
    pub plain: String,
    pub value: Value,
}

pub enum PendingState {
    Waiting,
    Done(Account),
    Failed(String),
}

pub struct Pending {
    pub previous: Option<Vec<u8>>,
    pub state: PendingState,
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    b.chunks(2)
        .map(|p| Some((hex_val(p[0])? << 4) | hex_val(p[1])?))
        .collect()
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

pub fn decode(raw: &[u8]) -> Result<Decoded, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "credential is not utf-8".to_string())?;
    let text = text.trim();
    let plain = if let Some(rest) = text.strip_prefix(B64_PREFIX) {
        let rest = rest.trim();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(rest)
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(rest.trim_end_matches('=')))
            .map_err(|_| "credential base64 is invalid".to_string())?;
        String::from_utf8(bytes).map_err(|_| "credential is not utf-8".to_string())?
    } else if let Some(rest) = text.strip_prefix(HEX_PREFIX) {
        let bytes = hex_decode(rest.trim()).ok_or_else(|| "credential hex is invalid".to_string())?;
        String::from_utf8(bytes).map_err(|_| "credential is not utf-8".to_string())?
    } else {
        text.to_string()
    };
    let plain = plain.trim().to_string();
    let value: Value = serde_json::from_str(&plain).map_err(|_| "credential is not json".to_string())?;
    if !value.is_object() {
        return Err("credential is not a json object".into());
    }
    Ok(Decoded { plain, value })
}

pub fn identity(raw: &[u8]) -> Option<String> {
    decode(raw).ok().and_then(|d| jwt::identity_of(&d.value))
}

pub fn email(raw: &[u8]) -> Option<String> {
    decode(raw).ok().and_then(|d| jwt::email_of(&d.value))
}

pub fn blob_for_write(raw: &[u8]) -> Result<Vec<u8>, String> {
    if cfg!(target_os = "macos") {
        let text = std::str::from_utf8(raw).map_err(|_| "credential is not utf-8".to_string())?;
        return Ok(text.trim_end_matches(['\n', '\r']).as_bytes().to_vec());
    }
    Ok(decode(raw)?.plain.into_bytes())
}

fn err(code: &'static str, message: impl Into<String>) -> ApiError {
    ApiError { code, message: message.into() }
}

fn io_err(message: String) -> ApiError {
    err("keyring", message)
}

impl Service {
    fn restore_previous(&self, previous: &Option<Vec<u8>>) {
        if let Some(prev) = previous {
            if let Ok(blob) = blob_for_write(prev) {
                let _ = self.io.live_set(&blob);
            }
        }
    }

    fn account_for_identity(&self, ident: &str) -> Option<AccountRec> {
        let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        store
            .accounts
            .iter()
            .find(|a| a.agent == "antigravity" && a.identity.as_deref() == Some(ident))
            .cloned()
    }

    fn sync_live(&self, live: &[u8]) -> Result<(), ApiError> {
        let Some(ident) = identity(live) else {
            return Ok(());
        };
        if let Some(acc) = self.account_for_identity(&ident) {
            self.io.snapshot_set(&acc.id, live).map_err(io_err)?;
        }
        Ok(())
    }

    fn save_login(&self, live: &[u8], ident: &str) -> Result<AccountRec, ApiError> {
        if let Some(existing) = self.account_for_identity(ident) {
            self.io.snapshot_set(&existing.id, live).map_err(io_err)?;
            return Ok(existing);
        }
        let email = email(live);
        let rec = AccountRec {
            agent: "antigravity".into(),
            id: super::new_id(),
            label: email.clone().unwrap_or_else(|| ident.to_string()),
            email,
            created_at: super::now_ms(),
            identity: Some(ident.to_string()),
        };
        self.io.snapshot_set(&rec.id, live).map_err(io_err)?;
        let mut store = self.store.lock().unwrap_or_else(|e| e.into_inner());
        store.accounts.push(rec.clone());
        if let Err(e) = super::save_store(&self.root, &store) {
            store.accounts.pop();
            self.io.snapshot_delete(&rec.id);
            return Err(err("store", e));
        }
        Ok(rec)
    }

    pub fn agy_add_start(&self) -> Result<(String, &'static str), ApiError> {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.retain(|_, p| matches!(p.state, PendingState::Waiting));
        if !pending.is_empty() {
            return Err(err("busy", "another login is already in progress"));
        }
        let live = self.io.live_get().map_err(io_err)?;
        if let Some(blob) = &live {
            self.sync_live(blob)?;
        }
        self.io.live_delete().map_err(io_err)?;
        let id = super::new_id();
        pending.insert(id.clone(), Pending { previous: live, state: PendingState::Waiting });
        Ok((id, "agy"))
    }

    pub fn spawn_poll(self: &std::sync::Arc<Self>, pending_id: String) {
        let svc = self.clone();
        std::thread::spawn(move || {
            let deadline = Instant::now() + svc.poll_timeout;
            loop {
                std::thread::sleep(svc.poll_every);
                if !svc.poll_step(&pending_id, Instant::now() >= deadline) {
                    break;
                }
            }
        });
    }

    pub fn poll_step(&self, pending_id: &str, timed_out: bool) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let Some(p) = pending.get_mut(pending_id) else {
            return false;
        };
        if !matches!(p.state, PendingState::Waiting) {
            return false;
        }
        let live = self.io.live_get().ok().flatten();
        let ident = live.as_deref().and_then(identity);
        let (Some(blob), Some(ident)) = (live, ident) else {
            if timed_out {
                self.restore_previous(&p.previous);
                p.state = PendingState::Failed("login timed out".into());
                return false;
            }
            return true;
        };
        match self.save_login(&blob, &ident) {
            Ok(rec) => {
                self.restore_previous(&p.previous);
                p.state = PendingState::Done(rec.into_account());
            }
            Err(e) => {
                self.restore_previous(&p.previous);
                p.state = PendingState::Failed(e.message);
            }
        }
        false
    }

    pub fn agy_add_status(&self, pending_id: &str) -> Value {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        match pending.get(pending_id).map(|p| &p.state) {
            Some(PendingState::Waiting) => json!({"state": "waiting"}),
            Some(PendingState::Done(acc)) => json!({"state": "done", "account": acc}),
            Some(PendingState::Failed(e)) => json!({"state": "failed", "error": e}),
            None => json!({"state": "failed", "error": "unknown login"}),
        }
    }

    pub fn agy_add_cancel(&self, pending_id: &str) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(p) = pending.remove(pending_id) {
            if matches!(p.state, PendingState::Waiting) {
                self.restore_previous(&p.previous);
            }
        }
    }

    pub fn agy_activate(&self, id: &str) -> Result<Value, ApiError> {
        {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            if !store.accounts.iter().any(|a| a.agent == "antigravity" && a.id == id) {
                return Err(err("not_found", "account not found"));
            }
        }
        let target = self
            .io
            .snapshot_get(id)
            .map_err(io_err)?
            .ok_or_else(|| err("not_found", "account credential is missing"))?;
        let write = blob_for_write(&target).map_err(|e| err("invalid_credential", e))?;
        if let Some(live) = self.io.live_get().map_err(io_err)? {
            self.sync_live(&live)?;
        }
        self.io.live_set(&write).map_err(io_err)?;
        if let Some(path) = &self.settings_path {
            let _ = crate::orch::agy::strip_model_provider(path);
        }
        Ok(json!({ "tiles": agy_tiles() }))
    }

    pub fn live_agy(&self) -> Option<String> {
        let live = self.io.live_get().ok().flatten()?;
        let ident = identity(&live)?;
        self.account_for_identity(&ident).map(|a| a.id)
    }
}

fn agy_tiles() -> Vec<Value> {
    use std::sync::atomic::Ordering;
    let sessions: HashMap<String, std::sync::Arc<crate::Session>> =
        crate::sessions().lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut out = Vec::new();
    for (tile_id, session) in sessions {
        if session.exited.load(Ordering::Relaxed) {
            continue;
        }
        let state = session.agent_state.lock().unwrap_or_else(|e| e.into_inner());
        if state.get("agent").and_then(Value::as_str) != Some("antigravity") {
            continue;
        }
        let status = state
            .get("lastStatus")
            .and_then(Value::as_str)
            .filter(|s| matches!(*s, "idle" | "busy" | "waiting"))
            .map(str::to_string);
        drop(state);
        let conversation = crate::read_binding_rec(&tile_id)
            .filter(|b| b.get("agent").and_then(Value::as_str) == Some("antigravity"))
            .and_then(|b| b.get("agentSessionId").and_then(Value::as_str).map(str::to_string));
        out.push(json!({ "tileId": tile_id, "conversationId": conversation, "status": status }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_plain_with_trailing_newline() {
        let d = decode(b"{\"email\":\"a@b.co\"}\n").unwrap();
        assert_eq!(d.plain, "{\"email\":\"a@b.co\"}");
    }

    #[test]
    fn decode_base64_envelope() {
        let enc = base64::engine::general_purpose::STANDARD.encode(b"{\"email\":\"a@b.co\"}");
        let raw = format!("{B64_PREFIX}{enc}\n");
        assert_eq!(decode(raw.as_bytes()).unwrap().plain, "{\"email\":\"a@b.co\"}");
    }

    #[test]
    fn decode_hex_envelope() {
        let hex: String = b"{\"a\":1}".iter().map(|b| format!("{b:02x}")).collect();
        let raw = format!("{HEX_PREFIX}{hex}");
        assert_eq!(decode(raw.as_bytes()).unwrap().plain, "{\"a\":1}");
    }

    #[test]
    fn decode_rejects_non_object() {
        assert!(decode(b"[1]").is_err());
        assert!(decode(b"nope").is_err());
        assert!(decode(format!("{HEX_PREFIX}zz").as_bytes()).is_err());
    }

    #[test]
    fn write_blob_unwraps_envelope_off_macos() {
        let enc = base64::engine::general_purpose::STANDARD.encode(b"{\"a\":1}");
        let raw = format!("{B64_PREFIX}{enc}");
        let out = blob_for_write(raw.as_bytes()).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(out, raw.as_bytes());
        } else {
            assert_eq!(out, b"{\"a\":1}");
        }
    }
}
