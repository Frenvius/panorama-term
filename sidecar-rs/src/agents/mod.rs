pub mod cli;
mod compact;
mod context;
mod delivery;
mod envelope;
mod identity;
mod launch;
mod mail;
mod memory;
mod messages;
mod model;
mod reducer;
mod rollover;
mod routine_hook;
mod routines;
mod runtime;
mod schedule;
mod store;
mod waits;

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

#[derive(Debug)]
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
    memory::init();
    messages::init();
    waits::init();
    routines::init();
    rollover::repair_after_boot();
    runtime::restore_all();
    compact::start();
}

pub(crate) fn compact_agent_logs(force: bool) -> compact::Sizes {
    let mut total = compact::Sizes::default();
    let Some(base) = crate::panorama_dir() else { return total };
    let ids: Vec<String> = lock().agents.keys().cloned().collect();
    for id in ids {
        let mut guard = lock();
        if !guard.agents.contains_key(&id) {
            continue;
        }
        let path = store::events_path(&base, &id);
        let seq = guard.seqs.get(&id).copied().unwrap_or(0);
        let since = seq.saturating_sub(guard.snaps.get(&id).copied().unwrap_or(0));
        if !force && !compact::needs_compaction(&path, since) {
            total.add(compact::unchanged(&path));
            continue;
        }
        let Ok(data) = serde_json::to_value(&guard.agents[&id]) else { continue };
        match compact::compact_log(&path, seq, "agent-snapshot", data) {
            Ok(sizes) => {
                total.add(sizes);
                guard.snaps.insert(id, seq);
            }
            Err(e) => eprintln!("[compact] agent {id}: {e}"),
        }
    }
    total
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
        "wait" => waits::handle_wait(&v),
        "waits" => waits::handle_waits(&v),
        "wait-cancel" => waits::handle_wait_cancel(&v),
        "coordinate" => handle_coordinate(&v).await,
        "routines" => routines::handle_list(&v),
        "routine-create" => routines::handle_create(&v),
        "routine-update" => routines::handle_update(&v),
        "routine-enable" => routines::handle_enable(&v, true),
        "routine-disable" => routines::handle_enable(&v, false),
        "routine-delete" => routines::handle_delete(&v),
        "routine-run" => routines::handle_run(&v, text_field(&v, "payload")),
        "routine-runs" => routines::handle_runs(&v),
        "routine-report" => routines::handle_report(&v),
        "routine-token" => routines::handle_token(&v),
        "routine-payload" => routines::handle_payload(&v),
        "memory" => memory::handle_list(&v),
        "memory-remember" => memory::handle_remember(&v),
        "memory-edit" => memory::handle_edit(&v),
        "memory-set" => memory::handle_set(&v),
        "memory-forget" => memory::handle_forget(&v),
        "memory-search" => memory::handle_search(&v),
        "handoff" => rollover::handle_handoff(&v),
        "rollover" => rollover::handle_rollover(&v),
        "compact" => tokio::task::spawn_blocking(move || compact::handle_compact(&v)).await.unwrap_or_else(|_| err_resp("io", "compaction failed")),
        _ => err_resp("bad_request", "unknown command"),
    }
}

pub(crate) fn ui_user_cap() -> &'static str {
    routine_hook::user_cap_for_ui()
}

pub(crate) const HOOK_PAYLOAD_MAX: usize = routine_hook::PAYLOAD_MAX_BYTES;
pub(crate) const HOOK_TOKEN_HEADER: &str = routine_hook::TOKEN_HEADER;

pub(crate) fn routine_hook_http(post: bool, routine: &str, token: Option<&str>, origin: bool, len: usize, body: &[u8]) -> (u16, &'static str, String) {
    let answer = if !post {
        routine_hook::HookAnswer::NotFound
    } else if len > HOOK_PAYLOAD_MAX {
        routine_hook::HookAnswer::TooLarge
    } else {
        routine_hook::handle_hook(routine, token, origin, body)
    };
    let (status, text) = answer.status();
    (status, text, answer.body())
}

pub(crate) fn spawn_background_checked<F>(future: F) -> bool
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    runtime::spawn_checked(future)
}

pub(crate) fn spawn_background<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    runtime::spawn_background(future);
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

pub(crate) fn block_parts(agent: &Agent) -> (String, Vec<context::RosterEntry>) {
    (context::stable_block(agent, &crate::panorama_cli_path(), None), roster_for(&agent.id))
}

#[cfg(test)]
pub(crate) fn full_block(agent: &Agent) -> String {
    let (stable, roster) = block_parts(agent);
    context::with_dynamic(&stable, &roster, &memory::essential_of(&agent.id), &crate::panorama_cli_path())
}

fn agent_for_tile(tile_id: &str) -> Option<Agent> {
    let id = model::agent_id_from_tile(tile_id)?;
    lock().agents.get(id).cloned()
}

pub(crate) fn prompt_context(tile_id: &str, session_id: &str, orch: Option<String>) -> Option<String> {
    let agent = agent_for_tile(tile_id)?;
    let cli = crate::panorama_cli_path();
    let stable = context::stable_block(&agent, &cli, orch.as_deref());
    context::prompt_block(stable, &roster_for(&agent.id), &memory::essential_of(&agent.id), &cli, tile_id, session_id)
}

fn session_key(agent: &Agent) -> String {
    match &agent.native_session {
        Some(s) => s.id.clone(),
        None => format!("launch-{}", agent.last_started_at.unwrap_or(0)),
    }
}

pub(crate) fn system_context(tile_id: &str) -> Option<String> {
    let agent = agent_for_tile(tile_id)?;
    let (stable, roster) = block_parts(&agent);
    let key = session_key(&agent);
    let frozen = context::frozen_roster(tile_id, &key, &roster);
    let memories = context::frozen_memories(tile_id, &key, &memory::essential_of(&agent.id));
    Some(context::with_dynamic(&stable, &frozen, &memories, &crate::panorama_cli_path()))
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
    let exe = profile_exe(&agent.profile)?;
    let memories = memory::essential_of(&agent.id);
    if exe == "pi" {
        return context::frozen_memory_delta(&agent.tile_id(), &session_key(&agent), &memories, &crate::panorama_cli_path());
    }
    if context::has_context_hook(&exe) {
        return None;
    }
    let (stable, roster) = block_parts(&agent);
    context::paste_prefix(&stable, &roster, &memories, &crate::panorama_cli_path(), &agent.tile_id())
}

pub(crate) fn mark_block_pasted(agent_id: &str) {
    let Some(agent) = lock().agents.get(agent_id).cloned() else { return };
    let memories = memory::essential_of(&agent.id);
    if profile_exe(&agent.profile).as_deref() == Some("pi") {
        context::set_memory_pasted(&agent.tile_id(), &session_key(&agent), &memories);
        return;
    }
    let (stable, roster) = block_parts(&agent);
    context::set_pasted(&agent.tile_id(), &stable, &roster, &memories);
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

pub(crate) fn on_orch_commit(run_id: &str) {
    waits::on_orch(run_id);
}

pub(crate) fn orch_notice(tile_id: &str, run_id: &str, text: &str) -> Result<(), String> {
    let id = model::agent_id_from_tile(tile_id).ok_or("not an agent tile")?;
    waits::orch_notice(id, run_id, text)
}

pub(crate) fn orch_wake(tile_id: &str, run_id: &str, top: u32, text: &str) {
    if let Some(id) = model::agent_id_from_tile(tile_id) {
        waits::orch_wake(id, run_id, top, text);
    }
}

const CLOSE_GRACE_S: u64 = 15;

pub(crate) fn on_session_exit(session: &str, code: Option<u32>, plain: bool) {
    waits::on_session_exit(session, code);
    if !plain || session.starts_with("agent-") {
        return;
    }
    if code.is_some() {
        on_tile_closed(session);
        return;
    }
    let tile = session.to_string();
    runtime::spawn_background(async move {
        tokio::time::sleep(std::time::Duration::from_secs(CLOSE_GRACE_S)).await;
        let gone = tokio::task::spawn_blocking({ let t = tile.clone(); move || crate::lookup_live(&t).is_none() && crate::host_session_alive(&t) == Some(false) }).await;
        if matches!(gone, Ok(true)) {
            on_tile_closed(&tile);
        }
    });
}

fn guard_closed() -> std::sync::MutexGuard<'static, std::collections::HashSet<String>> {
    static SET: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn on_tile_opened(tile_id: &str) {
    guard_closed().remove(tile_id);
}

pub(crate) fn tile_closed(tile_id: &str) -> bool {
    guard_closed().contains(tile_id)
}

pub(crate) fn on_tile_closed(tile_id: &str) {
    let party = mail::Party::Tile { id: tile_id.to_string() };
    let label = format!("terminal {}", tile_id.chars().take(8).collect::<String>());
    guard_closed().insert(tile_id.to_string());
    messages::drop_queued_for(&party, "tile_closed", &format!("{label} was closed"), true);
    waits::cancel_for(&party, "tile_closed");
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
    if let Some(open) = agent.rollover.open.as_ref().filter(|o| o.phase == "open") {
        let _ = commit_cmd(&id, Cmd::RolloverEnd { id: id.clone(), n: open.n, at: unix_ms(), outcome: "aborted:stopped".into() });
        crate::ui::broadcast_agents_rollover(&id);
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
        last_started_at: None, started_by: None, rollover: Default::default(),
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
        obj.insert("coordinates".into(), crate::orch::runs_coordinated_by(&agent.tile_id()).into());
        obj.insert("routines".into(), routines::summary_for(&agent.id));
        obj.insert("memory".into(), memory::summary_for(&agent.id));
        obj.insert("canCoordinate".into(), (profile_exe(&agent.profile).as_deref() == Some(COORDINATOR_EXE)).into());
        obj.insert("waits".into(), waits::open_waits_json(&mail::Party::Agent { id: agent.id.clone() }));
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
    if v.get("rolloverAt").is_some() && !identity::user_cap(v) {
        return err_resp("forbidden", "the renewal threshold is set by the user in the Panorama panel");
    }
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
    waits::cancel_for(&mail::Party::Agent { id: id.clone() }, "agent_deleted");
    detach_coordinator(tile.clone());
    routines::on_agent_deleted(&id);
    memory::on_agent_deleted(&id);
    rollover::forget(&id);
    identity::forget(&id);
    if let Err(e) = store::remove_binding(&base, &tile) {
        eprintln!("[agents] {e}");
    }
    crate::ui::broadcast_agents_changed(&id);
    ok_resp(json!({}))
}

pub(crate) const COORDINATOR_EXE: &str = "claude";

#[derive(Debug, PartialEq, Eq)]
pub enum FreshOutcome {
    Ready,
    Abandoned,
    Busy,
    Overlap,
    Failed(String),
}

#[cfg(test)]
pub(crate) async fn fresh_session_for(agent_id: &str, routine: &str, wanted: impl Fn() -> bool + Send + Sync) -> FreshOutcome {
    renew_for_routine(agent_id, routine, None, None, wanted).await
}

pub(crate) async fn renew_for_routine(agent_id: &str, routine: &str, run: Option<String>, rollover: Option<bool>, wanted: impl Fn() -> bool + Send + Sync) -> FreshOutcome {
    if !wanted() {
        return FreshOutcome::Abandoned;
    }
    let (note, handoff) = match rollover {
        Some(note) => (note, true),
        None => (false, false),
    };
    let started = unix_ms();
    loop {
        if !wanted() {
            return FreshOutcome::Abandoned;
        }
        let Some(agent) = lock().agents.get(agent_id).cloned() else { return FreshOutcome::Abandoned };
        if agent.rollover.open.is_some() {
            return FreshOutcome::Overlap;
        }
        let ready = <rollover::LiveBackend as rollover::Backend>::ready(&rollover::LiveBackend, agent_id);
        if rollover::can_start(&ready) || (!ready.live && agent.desired == Desired::Stopped) {
            break;
        }
        if unix_ms().saturating_sub(started) >= rollover::START_WAIT_MS {
            return FreshOutcome::Busy;
        }
        tokio::time::sleep(std::time::Duration::from_millis(rollover::WAIT_POLL_MS)).await;
    }
    let n = match rollover::begin(agent_id, &format!("routine:{routine}"), false, note, handoff, run) {
        Ok(n) => n,
        Err(e) if e.code == "conflict" => return FreshOutcome::Overlap,
        Err(e) => return FreshOutcome::Failed(format!("failed:{}", e.code)),
    };
    let outcome = rollover::drive(&rollover::LiveBackend, agent_id, n, &wanted).await;
    match outcome.as_str() {
        o if o.starts_with("done") => FreshOutcome::Ready,
        "aborted:unwanted" | "gone" => FreshOutcome::Abandoned,
        "aborted:busy" => FreshOutcome::Busy,
        _ => FreshOutcome::Failed(outcome),
    }
}

pub(crate) fn profile_exe_of(agent_id: &str) -> Option<String> {
    let profile = lock().agents.get(agent_id).map(|a| a.profile.clone())?;
    profile_exe(&profile)
}
async fn orch_call(command: &str, body: Value) -> Result<Value, String> {
    let text = crate::orch::handle(command, body.to_string().as_bytes()).await;
    let v: Value = serde_json::from_str(&text).unwrap_or_default();
    if v["ok"] == true { Ok(v) } else { Err(text) }
}

fn detach_coordinator(tile: String) {
    runtime::spawn_background(async move {
        for run in crate::orch::runs_coordinated_by(&tile) {
            if let Err(e) = orch_call("run-set-coordinator", json!({ "runId": run, "tileId": null })).await {
                eprintln!("[agents] detach {run}: {e}");
            }
        }
        let _ = orch_call("coordinator-set", json!({ "tileId": tile, "on": false })).await;
    });
}

async fn handle_coordinate(v: &Value) -> String {
    let key = text_field(v, "agent").or_else(|| text_field(v, "id")).unwrap_or_default();
    let Some(agent) = lock().resolve(&key).cloned() else { return err_resp("not_found", "agent not found") };
    let Some(run) = text_field(v, "runId").filter(|r| !r.is_empty()) else { return err_resp("bad_request", "runId required") };
    let on = v.get("on").and_then(Value::as_bool).unwrap_or(true);
    let tile = agent.tile_id();
    if on && profile_exe(&agent.profile).as_deref() != Some(COORDINATOR_EXE) {
        return err_resp("unsupported", "coordinator agents need claude");
    }
    let tile_value = if on { Value::String(tile.clone()) } else { Value::Null };
    if !on && !crate::orch::runs_coordinated_by(&tile).contains(&run) {
        return err_resp("conflict", &format!("{} does not coordinate {run}", agent.name));
    }
    if let Err(e) = orch_call("run-set-coordinator", json!({ "runId": run, "tileId": tile_value })).await {
        return e;
    }
    let runs = crate::orch::runs_coordinated_by(&tile);
    if !on && runs.is_empty() {
        let _ = orch_call("coordinator-set", json!({ "tileId": tile, "on": false })).await;
    }
    crate::ui::broadcast_agents_changed(&agent.id);
    let text = if on { format!("{} now coordinates {run}.", agent.name) } else { format!("{} no longer coordinates {run}.", agent.name) };
    ok_resp(json!({ "coordinates": runs, "text": text }))
}

#[cfg(test)]
pub(crate) fn handle_update_for_test(v: &Value) -> String {
    handle_update(v)
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
        let roster = roster_for(&me);
        remove_test_agent(&me);
        remove_test_agent(&other);
        assert!(roster.iter().any(|e| e.name == format!("peer-{other}") && e.title == "Reviewer"), "{roster:?}");
        assert!(!roster.iter().any(|e| e.name == format!("me-{me}")), "{roster:?}");
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
        assert!(after_roster_change.as_deref().is_some_and(|b| b.starts_with("[panorama agents] ") && (b.contains(&format!("new-{newcomer} (Reviewer)")) || b.contains("The agent list changed"))), "{after_roster_change:?}");
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
    fn pi_keeps_the_roster_of_its_session_until_the_session_changes() {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("apif{:08}", nanos % 100_000_000);
        let mut agent = reducer::tests::agent(&id, &format!("pi-{id}"));
        agent.native_session = Some(model::NativeSession { agent: "pi".into(), id: format!("s1-{id}"), bound_at: 1, model: None });
        lock().agents.insert(id.clone(), agent);
        let tile = model::tile_id(&id);
        let before = system_context(&tile).unwrap();
        let newcomer = format!("apig{:08}", nanos % 100_000_000);
        insert_test_agent(&newcomer, &format!("late-{newcomer}"));
        let same_session = system_context(&tile).unwrap();
        lock().agents.get_mut(&id).unwrap().native_session = Some(model::NativeSession { agent: "pi".into(), id: format!("s2-{id}"), bound_at: 2, model: None });
        let _ = system_context(&tile).unwrap();
        let frozen = crate::read_binding_rec(&tile).map(|r| r["agentRoster"].clone()).unwrap_or_default();
        remove_test_agent(&newcomer);
        remove_test_agent(&id);
        if let Some(path) = crate::binding_path(&tile) { let _ = std::fs::remove_file(path); }
        assert_eq!(before, same_session, "a roster edit does not change pi's system context mid-session");
        assert_eq!(frozen["sessionId"], format!("s2-{id}"));
        assert!(frozen["names"].as_array().unwrap().iter().any(|e| e["name"] == format!("late-{newcomer}")), "a new session freezes the new roster");
    }

    #[test]
    fn pi_freezes_essential_memories_per_session_and_gets_changes_as_a_mail_prefix() {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("apim{:08}", nanos % 100_000_000);
        let name = format!("pim-{id}");
        let mut agent = reducer::tests::agent(&id, &name);
        agent.profile = "pi".into();
        agent.native_session = Some(model::NativeSession { agent: "pi".into(), id: format!("s1-{id}"), bound_at: 1, model: None });
        store::ensure_agent_dir(&crate::panorama_dir().unwrap(), &id).unwrap();
        lock().agents.insert(id.clone(), agent);
        let tile = model::tile_id(&id);
        let cap = routines::tests::cap();
        let remember = |text: &str| serde_json::from_str::<Value>(&memory::handle_remember(&json!({ "agent": name, "text": text, "essential": true, "userCap": cap }))).unwrap();
        assert_eq!(remember("Reply in Portuguese.")["ok"], true);
        let before = system_context(&tile).unwrap();
        assert_eq!(remember("Build with bun.")["ok"], true);
        let same_session = system_context(&tile).unwrap();
        let prefix = pending_block_for_paste(&id);
        mark_block_pasted(&id);
        let after_mark = pending_block_for_paste(&id);
        let still = system_context(&tile).unwrap();
        lock().agents.get_mut(&id).unwrap().native_session = Some(model::NativeSession { agent: "pi".into(), id: format!("s2-{id}"), bound_at: 2, model: None });
        let next = system_context(&tile).unwrap();
        remove_test_agent(&id);
        memory::on_agent_deleted(&id);
        if let Some(path) = crate::binding_path(&tile) { let _ = std::fs::remove_file(path); }
        assert!(before.contains("by user] Reply in Portuguese.") && !before.contains("Build with bun."), "{before}");
        assert_eq!(before, same_session, "the system prompt is not rewritten mid-session");
        assert_eq!(before, still);
        assert!(prefix.as_deref().is_some_and(|p| p.starts_with("[panorama memory] added: [") && p.ends_with("by user] Build with bun.")), "{prefix:?}");
        assert_eq!(after_mark, None);
        assert!(next.contains("Build with bun."), "a new session freezes the new memories");
    }
    #[test]
    fn a_memory_added_mid_session_reaches_the_hook_as_one_delta_line() {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("apmd{:08}", nanos % 100_000_000);
        let name = format!("md-{id}");
        insert_test_agent(&id, &name);
        store::ensure_agent_dir(&crate::panorama_dir().unwrap(), &id).unwrap();
        let tile = model::tile_id(&id);
        let first = prompt_context(&tile, "sess-1", None);
        let quiet = prompt_context(&tile, "sess-1", None);
        let saved: Value = serde_json::from_str(&memory::handle_remember(&json!({ "agent": name, "text": "Reply in Portuguese.", "essential": true, "userCap": routines::tests::cap() }))).unwrap();
        let delta = prompt_context(&tile, "sess-1", None);
        let after = prompt_context(&tile, "sess-1", None);
        let compacted = { on_compact(&tile); prompt_context(&tile, "sess-1", None) };
        remove_test_agent(&id);
        memory::on_agent_deleted(&id);
        if let Some(path) = crate::binding_path(&tile) { let _ = std::fs::remove_file(path); }
        let roster_only = |o: &Option<String>| o.as_deref().is_none_or(|l| l.starts_with("[panorama agents]") && !l.contains("[panorama memory]"));
        assert!(first.is_some() && roster_only(&quiet), "{quiet:?}");
        assert_eq!(saved["ok"], true, "{saved}");
        let line = delta.unwrap();
        assert!(!line.starts_with("[panorama agent]"), "never the full block: {line}");
        assert_eq!(line.lines().last().unwrap(), format!("[panorama memory] added: [{} by user] Reply in Portuguese.", saved["id"].as_str().unwrap()));
        assert!(roster_only(&after), "{after:?}");
        let full = compacted.unwrap();
        assert!(full.starts_with("[panorama agent]") && full.contains(context::MEMORY_HEADER) && full.contains("Reply in Portuguese."), "a compact resends the block with memories");
    }
    #[test]
    fn a_coordinator_agent_gets_its_rules_once_and_a_roster_edit_as_one_line() {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("apco{:08}", nanos % 100_000_000);
        insert_test_agent(&id, &format!("co-{id}"));
        let tile = model::tile_id(&id);
        let rules = Some("[Panorama] Orchestrate mode is ON (rules)".to_string());
        let first = prompt_context(&tile, "sess-1", rules.clone());
        let second = prompt_context(&tile, "sess-1", rules.clone());
        let newcomer = format!("apcp{:08}", nanos % 100_000_000);
        insert_test_agent(&newcomer, &format!("rev-{newcomer}"));
        let delta = prompt_context(&tile, "sess-1", rules.clone());
        let after_delta = prompt_context(&tile, "sess-1", rules.clone());
        let new_session = prompt_context(&tile, "sess-2", rules.clone());
        remove_test_agent(&newcomer);
        remove_test_agent(&id);
        if let Some(path) = crate::binding_path(&tile) { let _ = std::fs::remove_file(path); }
        let first = first.unwrap();
        assert!(first.contains("Orchestrate mode is ON") && first.contains("Panorama agents you can message"), "{first}");
        let only_delta = |out: &Option<String>| out.as_deref().is_none_or(|o| o.starts_with("[panorama agents] ") && !o.contains("Orchestrate"));
        assert!(only_delta(&second), "once per session: {second:?}");
        let delta = delta.unwrap();
        assert!(delta.starts_with("[panorama agents] ") && (delta.contains(&format!("rev-{newcomer}")) || delta.contains("The agent list changed")) && !delta.contains("Orchestrate"), "{delta}");
        assert!(only_delta(&after_delta) && !after_delta.as_deref().unwrap_or("").contains(&format!("rev-{newcomer}")), "{after_delta:?}");
        assert!(new_session.unwrap().contains("Orchestrate mode is ON"));
    }

    #[test]
    fn system_context_returns_the_full_block_on_every_call() {
        let id = format!("apisys{}", unix_ms() % 100_000);
        let agent = reducer::tests::agent(&id, "pi-agent");
        lock().agents.insert(id.clone(), agent.clone());
        let tile = model::tile_id(&id);
        let first = system_context(&tile).unwrap();
        let second = system_context(&tile).unwrap();
        lock().agents.remove(&id);
        if let Some(path) = crate::binding_path(&tile) { let _ = std::fs::remove_file(path); }
        assert_eq!(first, second, "pi gets the same block on every call of a session");
        assert!(first.starts_with(&context::stable_block(&agent, &crate::panorama_cli_path(), None)) && first.contains("Panorama agents you can message"));
        assert_eq!(system_context("agent-missing"), None);
        assert_eq!(system_context("plain-tile"), None);
    }
}