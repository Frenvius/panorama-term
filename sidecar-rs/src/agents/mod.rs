pub mod cli;
mod context;
mod delivery;
mod envelope;
mod launch;
mod mail;
mod messages;
mod model;
mod reducer;
mod runtime;
mod store;

use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::agents::model::*;
use crate::agents::reducer::{AgentPatch, ApplyResult, Cmd, State};

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const LIST_TAIL_MAX: usize = 200;

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn lock() -> std::sync::MutexGuard<'static, State> {
    state().lock().unwrap_or_else(|e| e.into_inner())
}

fn gen_agent_id() -> String {
    let mut bytes = [0u8; 7];
    getrandom::getrandom(&mut bytes).expect("os rng");
    let mut id = String::from("a");
    id.extend(bytes.iter().map(|b| ID_ALPHABET[*b as usize % ID_ALPHABET.len()] as char));
    id
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ok_resp(extra: Value) -> String {
    let mut v = json!({ "ok": true });
    if let (Some(obj), Some(ext)) = (v.as_object_mut(), extra.as_object()) {
        obj.extend(ext.clone());
    }
    v.to_string()
}

fn err_resp(code: &str, message: &str) -> String {
    json!({ "ok": false, "error": { "code": code, "message": message } }).to_string()
}

fn parse_body(body: &[u8]) -> Result<Value, String> {
    if body.is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_slice(body).map_err(|_| err_resp("bad_request", "invalid JSON body"))
}

struct CommitError {
    code: String,
    message: String,
}

impl CommitError {
    fn resp(&self) -> String {
        err_resp(&self.code, &self.message)
    }
}

fn commit_locked(guard: &mut State, agent_id: &str, cmd: &Cmd) -> Result<Option<(String, Value)>, CommitError> {
    let (kind, data) = match guard.apply(cmd) {
        ApplyResult::Ok(kind, data) => (kind, data),
        ApplyResult::Unchanged => return Ok(None),
        ApplyResult::Err(e) => return Err(CommitError { code: e.code.into(), message: e.message }),
    };
    let io = |message: String| CommitError { code: "io".into(), message };
    let base = crate::panorama_dir().ok_or_else(|| io("no panorama dir".into()))?;
    store::ensure_agent_dir(&base, agent_id).map_err(io)?;
    let seq = guard.seqs.get(agent_id).copied().unwrap_or(0) + 1;
    let record = store::EventRecord { seq, at: unix_ms(), kind: kind.clone(), data: data.clone() };
    store::append_event(&store::events_path(&base, agent_id), &record).map_err(io)?;
    if let Err(e) = guard.replay(&kind, &data) {
        eprintln!("[agents] replay after commit {kind}: {e}");
    }
    guard.seqs.insert(agent_id.to_string(), seq);
    Ok(Some((kind, data)))
}

fn commit_cmd(agent_id: &str, cmd: Cmd) -> Result<Option<(String, Value)>, CommitError> {
    let result = commit_locked(&mut lock(), agent_id, &cmd);
    if matches!(result, Ok(Some(_))) {
        crate::ui::broadcast_agents_changed(agent_id);
    }
    result
}

pub fn init() {
    let Some(base) = crate::panorama_dir() else { return };
    store::load_all(&mut lock(), &base);
    messages::init();
    runtime::restore_all();
}

pub async fn handle(command: &str, body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    match command {
        "create" => handle_create(&v),
        "list" => handle_list(&v),
        "show" => handle_show(&v),
        "update" => handle_update(&v),
        "delete" => handle_delete(&v),
        "start" => handle_start(&v),
        "stop" => handle_stop(&v).await,
        "send" => messages::handle_send(&v),
        "reply" => messages::handle_reply(&v),
        "messages" => messages::handle_messages(&v),
        "read" => messages::handle_read(&v),
        "mark-read" => messages::handle_mark_read(&v),
        _ => err_resp("bad_request", "unknown command"),
    }
}

pub(crate) fn is_agent_tile(tile_id: &str) -> bool {
    model::agent_id_from_tile(tile_id).is_some_and(|id| lock().agents.contains_key(id))
}

pub(crate) fn on_native_session_bound(tile_id: &str, agent: &str, session_id: &str) {
    let Some(record) = agent_for_tile(tile_id) else { return };
    let cli = launch::session_cli(agent);
    if profile_exe(&record.profile).as_deref() != Some(cli) {
        return;
    }
    runtime::bind_session(&record.id, cli, session_id);
}

fn profile_exe(profile_id: &str) -> Option<String> {
    let base = crate::panorama_dir()?;
    crate::orch::profiles::find_profile(&crate::orch::profiles::load_profiles(&base), profile_id).map(|p| p.exe.clone())
}

fn roster_for(agent_id: &str) -> Vec<context::RosterEntry> {
    lock().agents.values()
        .filter(|a| a.id != agent_id)
        .map(|a| context::RosterEntry { name: a.name.clone(), title: a.title.clone() })
        .collect()
}

pub(crate) fn full_block(agent: &Agent) -> String {
    context::with_roster(&context::agent_block(agent), &roster_for(&agent.id), &crate::panorama_cli_path())
}

fn agent_for_tile(tile_id: &str) -> Option<Agent> {
    let id = model::agent_id_from_tile(tile_id)?;
    lock().agents.get(id).cloned()
}

pub(crate) fn prompt_context(tile_id: &str, session_id: &str, extra: Option<String>) -> Option<String> {
    let agent = agent_for_tile(tile_id)?;
    let mut block = full_block(&agent);
    if let Some(extra) = extra.filter(|e| !e.is_empty()) {
        block.push_str("\n\n");
        block.push_str(&extra);
    }
    context::prompt_block(block, tile_id, session_id)
}

pub(crate) fn system_context(tile_id: &str) -> Option<String> {
    agent_for_tile(tile_id).map(|agent| full_block(&agent))
}

pub(crate) fn on_compact(tile_id: &str) {
    if is_agent_tile(tile_id) {
        context::forget_delivery(tile_id);
    }
}

pub(crate) fn delivery_view(agent_id: &str) -> Option<delivery::View> {
    let agent = lock().agents.get(agent_id).cloned()?;
    let live = live_view(&agent);
    Some(delivery::View {
        tile: live.tile_id,
        live: live.live,
        starting: runtime::is_starting(agent_id),
        status: live.status,
        source: live.status_source,
    })
}

pub(crate) fn pending_block_for_paste(agent_id: &str) -> Option<String> {
    let agent = lock().agents.get(agent_id).cloned()?;
    if context::has_context_hook(&profile_exe(&agent.profile)?) {
        return None;
    }
    let block = full_block(&agent);
    (context::pasted_hash(&agent.tile_id()).as_deref() != Some(context::block_hash(&block).as_str())).then_some(block)
}

pub(crate) fn mark_block_pasted(agent_id: &str) {
    let Some(agent) = lock().agents.get(agent_id).cloned() else { return };
    context::set_pasted_hash(&agent.tile_id(), &context::block_hash(&full_block(&agent)));
}

pub(crate) fn quiet_busy(tile_id: &str) -> bool {
    runtime::quiet_status(tile_id) == Some(LiveStatus::Busy)
}

pub(crate) fn on_tile_status(tile_id: &str, status: &str) {
    if !tile_id.starts_with("agent-") {
        let key = mail::Party::Tile { id: tile_id.to_string() }.key();
        messages::note_status(&key, status == "busy", status == "idle");
    }
}

pub(crate) fn on_tile_idle(tile_id: &str) {
    if !tile_id.starts_with("agent-") {
        delivery::kick(&mail::Party::Tile { id: tile_id.to_string() });
    }
}

pub(crate) fn on_status(tile_id: &str) {
    if let Some(id) = model::agent_id_from_tile(tile_id) {
        runtime::status_touched(id);
    }
}

fn start_agent(id: &str, started_by: Option<String>) -> Result<(), CommitError> {
    let live = crate::lookup_live(&model::tile_id(id)).is_some();
    let started = commit_cmd(id, Cmd::Start { id: id.into(), at: unix_ms(), live, started_by })?;
    if started.is_some() || !live {
        runtime::spawn_start(id.to_string());
    }
    Ok(())
}

fn handle_start(v: &Value) -> String {
    let id = match resolve_id(v) { Ok(id) => id, Err(e) => return e };
    if v.get("fresh").and_then(Value::as_bool) == Some(true) {
        if crate::lookup_live(&model::tile_id(&id)).is_some() {
            return err_resp("conflict", "agent is live; stop it before a fresh start");
        }
        let cmd = Cmd::SessionClear { id: id.clone(), at: unix_ms(), reason: "user".into() };
        if let Err(e) = commit_cmd(&id, cmd) {
            return e.resp();
        }
    }
    if let Err(e) = start_agent(&id, None) {
        return e.resp();
    }
    let live = crate::lookup_live(&model::tile_id(&id)).is_some();
    ok_resp(json!({ "agent": agent_json(&id, false), "live": live }))
}

fn stop_refusal(live: &AgentLive, force: bool) -> Option<&'static str> {
    if force {
        return None;
    }
    match (live.status, live.status_source) {
        (LiveStatus::Busy | LiveStatus::Waiting, _) => Some("agent busy; use force to stop it"),
        (LiveStatus::Idle, StatusSource::None) if live.live => Some("agent status unknown (no status source yet); use force to stop it"),
        _ => None,
    }
}

async fn handle_stop(v: &Value) -> String {
    let id = match resolve_id(v) { Ok(id) => id, Err(e) => return e };
    let force = v.get("force").and_then(Value::as_bool) == Some(true);
    let Some(agent) = lock().agents.get(&id).cloned() else { return err_resp("not_found", "agent not found") };
    if let Some(reason) = stop_refusal(&live_view(&agent), force) {
        return err_resp("conflict", reason);
    }
    let cmd = Cmd::Stop { id: id.clone(), at: unix_ms(), by: "user".into(), force, crashed: false };
    if let Err(e) = commit_cmd(&id, cmd) {
        return e.resp();
    }
    let tile = agent.tile_id();
    let stopped = runtime::stop_session(&tile).await;
    runtime::forget(&id, &tile);
    crate::ui::broadcast_agents_changed(&id);
    if !stopped {
        return err_resp("io", "terminal did not exit in time");
    }
    ok_resp(json!({ "agent": agent_json(&id, false) }))
}

fn known_profiles() -> Vec<String> {
    crate::panorama_dir()
        .map(|base| crate::orch::profiles::load_profiles(&base).into_iter().map(|p| p.id).collect())
        .unwrap_or_default()
}

fn normalize_cwd(raw: &str) -> Result<(String, Option<String>), String> {
    let path = crate::native_path(raw.trim());
    if !path.is_absolute() || !path.is_dir() {
        return Err("cwd must be an existing absolute directory".into());
    }
    let project = crate::repo_root(&path).map(crate::path_string);
    Ok((crate::path_string(&path), project))
}

fn text_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn resolve_id(v: &Value) -> Result<String, String> {
    let key = text_field(v, "id").or_else(|| text_field(v, "name")).filter(|k| !k.is_empty())
        .ok_or_else(|| err_resp("bad_request", "id or name required"))?;
    lock().resolve(&key).map(|a| a.id.clone()).ok_or_else(|| err_resp("not_found", "agent not found"))
}

fn handle_create(v: &Value) -> String {
    let name = text_field(v, "name").unwrap_or_default();
    let profile = text_field(v, "profile").unwrap_or_default();
    if name.is_empty() || profile.is_empty() {
        return err_resp("bad_request", "name and profile required");
    }
    let (cwd, project) = match normalize_cwd(&text_field(v, "cwd").unwrap_or_default()) {
        Ok(pair) => pair,
        Err(e) => return err_resp("bad_request", &e),
    };
    let now = unix_ms();
    let agent = Agent {
        id: gen_agent_id(),
        name,
        title: text_field(v, "title").unwrap_or_default(),
        description: text_field(v, "description").unwrap_or_default(),
        profile,
        model: text_field(v, "model").filter(|s| !s.is_empty()),
        effort: text_field(v, "effort").filter(|s| !s.is_empty()),
        cwd,
        project,
        instructions: text_field(v, "instructions").unwrap_or_default(),
        auto_start: v.get("autoStart").and_then(Value::as_bool).unwrap_or(true),
        desired: Desired::Stopped,
        native_session: None,
        last_exit: None,
        restarts: vec![],
        crashed: false,
        created_at: now,
        updated_at: now,
        last_started_at: None, started_by: None,
    };
    let id = agent.id.clone();
    let auto_start = agent.auto_start;
    if let Err(e) = commit_cmd(&id, Cmd::Create { agent, known_profiles: known_profiles() }) {
        return e.resp();
    }
    if auto_start {
        if let Err(e) = start_agent(&id, None) {
            return e.resp();
        }
    }
    ok_resp(json!({ "agent": agent_json(&id, true) }))
}

fn live_view(agent: &Agent) -> AgentLive {
    let tile_id = agent.tile_id();
    let session = crate::lookup_live(&tile_id);
    let state: Value = session.as_ref()
        .and_then(|s| crate::cached_agent_state(s))
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null);
    let hook_status = state["lastStatus"].as_str().and_then(|s| match s {
        "idle" => Some(LiveStatus::Idle),
        "busy" => Some(LiveStatus::Busy),
        "waiting" => Some(LiveStatus::Waiting),
        _ => None,
    });
    let live = session.is_some();
    let quiet = runtime::quiet_status(&tile_id);
    let (status, status_source) = match (live, hook_status, quiet) {
        _ if runtime::is_starting(&agent.id) => (LiveStatus::Starting, StatusSource::None),
        (true, Some(status), _) => (status, StatusSource::Hook),
        (true, None, Some(status)) => (status, StatusSource::Quiet),
        (true, None, None) => (LiveStatus::Idle, StatusSource::None),
        (false, ..) if agent.crashed => (LiveStatus::Crashed, StatusSource::None),
        (false, ..) if agent.desired == Desired::Running => (LiveStatus::Starting, StatusSource::None),
        (false, ..) => (LiveStatus::Stopped, StatusSource::None),
    };
    AgentLive {
        tile_id,
        status,
        live,
        status_source,
        context_pct: state["contextPercent"].as_f64().map(|p| p.clamp(0.0, 100.0).round() as u8),
        cost_usd: state["costUsd"].as_f64(),
    }
}

fn merge_live(agent: &Agent, full: bool) -> Value {
    let mut value = serde_json::to_value(agent).unwrap_or_default();
    let live = serde_json::to_value(live_view(agent)).unwrap_or_default();
    if let (Some(obj), Some(live)) = (value.as_object_mut(), live.as_object()) {
        if !full {
            obj.remove("instructions");
            obj.remove("restarts");
        }
        obj.extend(live.clone());
        let (queued, awaiting) = messages::mail_counts(&agent.id);
        obj.insert("queued".into(), queued.into());
        obj.insert("awaitingReplies".into(), awaiting.into());
    }
    value
}

fn agent_json(id: &str, full: bool) -> Value {
    let agent = lock().agents.get(id).cloned();
    agent.map(|a| merge_live(&a, full)).unwrap_or(Value::Null)
}

fn handle_list(v: &Value) -> String {
    let project = text_field(v, "project").filter(|p| !p.is_empty());
    let agents: Vec<Agent> = lock().agents.values()
        .filter(|a| project.as_deref().is_none_or(|p| a.project.as_deref().is_some_and(|ap| ap.eq_ignore_ascii_case(p))))
        .cloned()
        .collect();
    let list: Vec<Value> = agents.iter().map(|a| merge_live(a, false)).collect();
    ok_resp(json!({ "agents": list }))
}

fn handle_show(v: &Value) -> String {
    let id = match resolve_id(v) { Ok(id) => id, Err(e) => return e };
    let mut agent = agent_json(&id, true);
    if let Some(lines) = v.get("tail").and_then(Value::as_u64).filter(|n| *n > 0) {
        let tile = model::tile_id(&id);
        let text = crate::orch::worker::capture_worker_screen(&tile, (lines as usize).min(LIST_TAIL_MAX));
        agent["tail"] = text.into();
    }
    ok_resp(json!({ "agent": agent }))
}

fn patch_from_body(v: &Value) -> Result<AgentPatch, String> {
    let mut patch = reducer::patch_from_json(v);
    patch.project = None;
    if v.get("id").is_none() {
        patch.name = None;
    }
    if let Some(raw) = patch.cwd.take() {
        let (cwd, project) = normalize_cwd(&raw).map_err(|e| err_resp("bad_request", &e))?;
        patch.cwd = Some(cwd);
        patch.project = Some(project);
    }
    Ok(patch)
}

fn needs_restart(before: &Agent, after: &Agent) -> bool {
    before.profile != after.profile || before.model != after.model
        || before.effort != after.effort || before.cwd != after.cwd
}

fn handle_update(v: &Value) -> String {
    let id = match resolve_id(v) { Ok(id) => id, Err(e) => return e };
    let patch = match patch_from_body(v) { Ok(p) => p, Err(e) => return e };
    let before = lock().agents.get(&id).cloned();
    let cmd = Cmd::Update { id: id.clone(), patch, known_profiles: known_profiles(), at: unix_ms() };
    if let Err(e) = commit_cmd(&id, cmd) {
        return e.resp();
    }
    let after = lock().agents.get(&id).cloned();
    let restart_needed = match (&before, &after) {
        (Some(b), Some(a)) => needs_restart(b, a) && crate::lookup_live(&a.tile_id()).is_some(),
        _ => false,
    };
    ok_resp(json!({ "agent": agent_json(&id, true), "restartNeeded": restart_needed }))
}

fn handle_delete(v: &Value) -> String {
    let id = match resolve_id(v) { Ok(id) => id, Err(e) => return e };
    let Some(base) = crate::panorama_dir() else { return err_resp("io", "no panorama dir") };
    let tile = model::tile_id(&id);
    let live = crate::lookup_live(&tile).is_some();
    let name = {
        let mut guard = lock();
        let Some(agent) = guard.agents.get(&id) else { return err_resp("not_found", "agent not found") };
        if agent.desired == Desired::Running || live {
            return err_resp("conflict", "agent running; stop it first");
        }
        let name = agent.name.clone();
        if let Err(e) = store::remove_agent_dir(&base, &id) {
            return err_resp("io", &e);
        }
        guard.agents.remove(&id);
        guard.seqs.remove(&id);
        name
    };
    runtime::forget(&id, &tile);
    messages::on_agent_deleted(&id, &name);
    if let Err(e) = store::remove_binding(&base, &tile) {
        eprintln!("[agents] {e}");
    }
    crate::ui::broadcast_agents_changed(&id);
    ok_resp(json!({}))
}

#[cfg(test)]
pub(crate) fn insert_test_agent(id: &str, name: &str) {
    lock().agents.insert(id.to_string(), reducer::tests::agent(id, name));
}

#[cfg(test)]
pub(crate) fn remove_test_agent(id: &str) {
    lock().agents.remove(id);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(status: LiveStatus, status_source: StatusSource, is_live: bool) -> AgentLive {
        AgentLive { tile_id: "agent-x".into(), status, live: is_live, status_source, context_pct: None, cost_usd: None }
    }

    #[test]
    fn stop_needs_force_when_busy_or_status_unknown() {
        assert!(stop_refusal(&live(LiveStatus::Busy, StatusSource::Hook, true), false).is_some());
        assert!(stop_refusal(&live(LiveStatus::Waiting, StatusSource::Hook, true), false).is_some());
        assert!(stop_refusal(&live(LiveStatus::Idle, StatusSource::None, true), false).is_some());
        assert!(stop_refusal(&live(LiveStatus::Idle, StatusSource::Hook, true), false).is_none());
        assert!(stop_refusal(&live(LiveStatus::Idle, StatusSource::Quiet, true), false).is_none());
        assert!(stop_refusal(&live(LiveStatus::Starting, StatusSource::None, false), false).is_none());
        assert!(stop_refusal(&live(LiveStatus::Busy, StatusSource::Hook, true), true).is_none());
    }

    fn body(v: Value) -> Value {
        serde_json::from_str(&v.to_string()).unwrap()
    }

    fn reply(text: String) -> Value {
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn handlers_rename_by_id_reject_injected_models_and_refuse_deleting_a_running_agent() {
        crate::orch::tests::f12_test_home();
        let cwd = std::env::temp_dir().to_string_lossy().into_owned();
        let name = format!("h{}", unix_ms() % 1_000_000);
        let created = reply(handle_create(&body(json!({ "name": name, "profile": "claude", "cwd": cwd, "autoStart": false }))));
        let id = created["agent"]["id"].as_str().unwrap().to_string();

        let renamed = reply(handle_update(&body(json!({ "id": id, "name": format!("{name}-2") }))));
        assert_eq!(renamed["agent"]["name"], format!("{name}-2"));
        let by_name = reply(handle_update(&body(json!({ "name": format!("{name}-2"), "title": "T" }))));
        assert_eq!(by_name["agent"]["name"], format!("{name}-2"), "name alone identifies, it does not rename");

        let injected = reply(handle_update(&body(json!({ "id": id, "model": "haiku;calc" }))));
        assert_eq!(injected["error"]["code"], "bad_request");
        let injected_create = reply(handle_create(&body(json!({ "name": format!("{name}-x"), "profile": "claude", "cwd": cwd, "model": "x $(calc)", "autoStart": false }))));
        assert_eq!(injected_create["error"]["code"], "bad_request");

        lock().agents.get_mut(&id).unwrap().desired = Desired::Running;
        let refused = reply(handle_delete(&body(json!({ "id": id }))));
        assert_eq!(refused["error"]["code"], "conflict");
        lock().agents.get_mut(&id).unwrap().desired = Desired::Stopped;
        let deleted = reply(handle_delete(&body(json!({ "id": id }))));
        assert_eq!(deleted["ok"], true);
        assert!(!lock().agents.contains_key(&id));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_status_reported_from_a_plain_thread_reaches_the_ui() {
        let id = format!("aosc{:04}", unix_ms() % 10_000);
        insert_test_agent(&id, "osc");
        runtime::remember_runtime(tokio::runtime::Handle::current());
        let mut events = crate::ui::subscribe_events();
        let tile = model::tile_id(&id);
        std::thread::spawn(move || on_status(&tile)).join().unwrap();
        let expected = format!("\"agentId\":\"{id}\"");
        let seen = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                match events.recv().await {
                    Ok(text) if text.contains("agents.changed") && text.contains(&expected) => return true,
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return false,
                }
            }
        }).await.unwrap_or(false);
        remove_test_agent(&id);
        assert!(seen, "no agents.changed for a status posted from a std::thread");
    }

    #[test]
    fn full_block_lists_the_other_agents_but_never_itself() {
        let tag = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 1_000_000_000;
        let me = format!("arost{tag:09}");
        let other = format!("arosu{tag:09}");
        insert_test_agent(&me, &format!("me-{me}"));
        insert_test_agent(&other, &format!("peer-{other}"));
        let agent = lock().agents[&me].clone();
        let block = full_block(&agent);
        remove_test_agent(&me);
        remove_test_agent(&other);
        assert!(block.contains(&format!("- peer-{other}: Reviewer")), "{block}");
        assert!(!block.contains(&format!("- me-{me}")), "{block}");
        assert!(block.contains("agent send <name>"));
    }

    #[test]
    fn no_hook_clis_get_the_block_in_front_of_mail_once_per_change() {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("ap{:06}", nanos % 1_000_000);
        let mut agent = reducer::tests::agent(&id, &format!("k-{id}"));
        agent.profile = "kimi".into();
        lock().agents.insert(id.clone(), agent);
        let first = pending_block_for_paste(&id);
        let marked_quiet = (0..50).any(|_| {
            mark_block_pasted(&id);
            pending_block_for_paste(&id).is_none()
        });
        let newcomer = format!("apasu{:03}", unix_ms() % 1000);
        insert_test_agent(&newcomer, &format!("new-{newcomer}"));
        let after_roster_change = pending_block_for_paste(&id);
        remove_test_agent(&newcomer);
        remove_test_agent(&id);
        if let Some(path) = crate::binding_path(&model::tile_id(&id)) { let _ = std::fs::remove_file(path); }
        assert!(first.is_some());
        assert!(marked_quiet, "once marked, the same block is not pasted again");
        assert!(after_roster_change.is_some_and(|b| b.contains(&format!("- new-{newcomer}"))));
        let claude = format!("apasv{:03}", unix_ms() % 1000);
        insert_test_agent(&claude, &format!("c-{claude}"));
        let hooked = pending_block_for_paste(&claude);
        remove_test_agent(&claude);
        assert!(hooked.is_none(), "CLIs with a context hook get the block through the hook");
    }
    #[test]
    fn a_plain_tile_cause_ends_with_its_own_busy_then_idle() {
        let tile = format!("tile-cause-{}", unix_ms());
        let key = mail::Party::Tile { id: tile.clone() }.key();
        messages::set_cause(&key, "m1", 1);
        on_tile_status(&tile, "busy");
        on_tile_status(&tile, "idle");
        assert!(messages::cause_of(&key).is_none());
    }
    #[test]
    fn system_context_returns_the_full_block_on_every_call() {
        let id = format!("apisys{}", unix_ms() % 100_000);
        let agent = reducer::tests::agent(&id, "pi-agent");
        lock().agents.insert(id.clone(), agent.clone());
        let tile = model::tile_id(&id);
        let stable = (0..50).any(|_| {
            let expected = full_block(&agent);
            let first = system_context(&tile);
            first.as_deref() == Some(expected.as_str()) && first == system_context(&tile)
        });
        lock().agents.remove(&id);
        assert!(stable, "pi gets the same full block on every call while the roster is unchanged");
        assert_eq!(system_context("agent-missing"), None);
        assert_eq!(system_context("plain-tile"), None);
    }
}