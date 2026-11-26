use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::agents::routine_hook::{digest, digests_equal, new_token};
use crate::agents::{err_resp, lock, store, text_field};

const TOKEN_FILE: &str = "session-token.sha256";

fn digests() -> std::sync::MutexGuard<'static, HashMap<String, String>> {
    static MAP: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

fn token_path(agent_id: &str) -> Option<std::path::PathBuf> {
    crate::panorama_dir().map(|b| store::agent_dir(&b, agent_id).join(TOKEN_FILE))
}

pub(crate) fn issue(agent_id: &str) -> String {
    let token = new_token();
    let hashed = digest(&token);
    if let Some(path) = token_path(agent_id) {
        if let Err(e) = std::fs::write(&path, hashed.as_bytes()) {
            eprintln!("[agents] session token for {agent_id} not saved: {e}");
        }
    }
    digests().insert(agent_id.to_string(), hashed);
    token
}

pub(crate) fn forget(agent_id: &str) {
    digests().remove(agent_id);
}

pub(crate) fn verify(agent_id: &str, presented: Option<&str>) -> bool {
    let Some(presented) = presented.filter(|p| !p.is_empty()) else { return false };
    let cached = digests().get(agent_id).cloned();
    let known = cached.or_else(|| {
        let saved = std::fs::read_to_string(token_path(agent_id)?).ok()?.trim().to_string();
        digests().insert(agent_id.to_string(), saved.clone());
        Some(saved)
    });
    known.is_some_and(|k| digests_equal(&k, &digest(presented)))
}

pub(crate) fn token_matches(v: &Value) -> Option<String> {
    let agent = text_field(v, "fromAgent")?;
    verify(&agent, text_field(v, "agentToken").as_deref()).then_some(agent)
}

pub(crate) fn live_agent(v: &Value, refusal: &str) -> Result<String, String> {
    if v.get("worker").and_then(Value::as_bool) == Some(true) {
        return Err(err_resp("forbidden", refusal));
    }
    let Some(agent) = token_matches(v) else { return Err(err_resp("forbidden", refusal)) };
    let own_tile = crate::agents::model::tile_id(&agent);
    let live = text_field(v, "fromTile").is_some_and(|t| t == own_tile && crate::lookup_live(&t).is_some());
    if !live || !lock().agents.contains_key(&agent) {
        return Err(err_resp("forbidden", refusal));
    }
    Ok(agent)
}

pub(crate) fn user_cap(v: &Value) -> bool {
    v.get("userCap").and_then(Value::as_str).is_some_and(crate::agents::routine_hook::user_cap_matches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_issued_token_matches_and_a_new_launch_replaces_it() {
        crate::orch::tests::f12_test_home();
        let id = format!("aid{:05}", crate::agents::unix_ms() % 100_000);
        store::ensure_agent_dir(&crate::panorama_dir().unwrap(), &id).unwrap();
        let first = issue(&id);
        assert!(verify(&id, Some(&first)));
        assert!(!verify(&id, Some("guess")) && !verify(&id, None) && !verify(&id, Some("")));
        assert!(!verify("aother00", Some(&first)), "a token is bound to its agent");
        let second = issue(&id);
        assert!(verify(&id, Some(&second)) && !verify(&id, Some(&first)));
        forget(&id);
        assert!(verify(&id, Some(&second)), "a brain restart reads the saved digest");
    }

    #[test]
    fn the_panel_capability_goes_only_to_local_origins() {
        assert!(crate::ui::capability_message(None).is_none(), "a script without Origin");
        assert!(crate::ui::capability_message(Some("http://evil.example")).is_none());
        assert!(crate::ui::capability_message(Some("http://127.0.0.1:9977")).is_none());
        let sent = crate::ui::capability_message(Some("tauri://localhost")).unwrap();
        assert!(sent.contains("ui.capability"));
    }

    #[test]
    fn user_only_routes_refuse_callers_without_the_capability() {
        crate::orch::tests::f12_test_home();
        let reply = |s: String| serde_json::from_str::<Value>(&s).unwrap();
        assert_eq!(reply(crate::agents::compact::handle_compact(&serde_json::json!({})))["error"]["code"], "forbidden");
        let id = format!("auc{:05}", crate::agents::unix_ms() % 100_000);
        lock().agents.insert(id.clone(), crate::agents::reducer::tests::agent(&id, &format!("u-{id}")));
        let update = reply(crate::agents::handle_update_for_test(&serde_json::json!({ "id": id, "rolloverAt": 0 })));
        assert_eq!(update["error"]["code"], "forbidden");
        let renew = reply(crate::agents::rollover::handle_rollover(&serde_json::json!({ "id": id, "fromTile": "tile-x" })));
        assert_eq!(renew["error"]["code"], "forbidden", "no env and no capability");
        let with_cap = reply(crate::agents::rollover::handle_rollover(&serde_json::json!({ "id": id, "userCap": crate::agents::routines::tests::cap() })));
        assert_eq!(with_cap["error"]["code"], "conflict", "past the check: the agent is not running");
        lock().agents.remove(&id);
    }
}