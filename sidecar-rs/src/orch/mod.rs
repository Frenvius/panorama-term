mod model;
mod model_catalog;
mod reducer;
mod store;
pub mod ai;
pub mod cli;
pub mod exec;
pub mod git;
pub mod profiles;
pub mod secrets;
pub mod settings;
pub mod worker;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use serde_json::Value;
use crate::orch::model::*;
use crate::orch::reducer::{ApplyResult, Cmd, State};
use crate::orch::store::{EventRecord, append_event, ensure_run_dir, load_into, run_events_path};
use crate::orch::profiles::{load_profiles, find_profile, resolve_env};
use crate::orch::worker::{BriefInput, WorkerLaunch, write_brief, spawn_worker, capture_worker_screen, maybe_nudge_tile, maybe_nudge_worker, is_tile_waiting};
use crate::ui::TileGroup;

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static NOTIFIES: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Notify>>>> = OnceLock::new();
static COORDINATORS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static STARTING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn notifies() -> &'static Mutex<HashMap<String, Arc<tokio::sync::Notify>>> {
    NOTIFIES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn coordinators() -> &'static Mutex<HashSet<String>> {
    COORDINATORS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn starting() -> &'static Mutex<HashSet<String>> {
    STARTING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn coordinators_path(base: &std::path::Path) -> std::path::PathBuf {
    base.join("orch").join("coordinators.json")
}

fn load_coordinators(base: &std::path::Path) -> HashSet<String> {
    std::fs::read_to_string(coordinators_path(base))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
        .map(|v| v.into_iter().collect())
        .unwrap_or_default()
}

fn save_coordinators(base: &std::path::Path, set: &HashSet<String>) -> Result<(), String> {
    let vec: Vec<&String> = set.iter().collect();
    let json = serde_json::to_string(&vec).map_err(|e| e.to_string())?;
    let path = coordinators_path(base);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

fn run_notify(run_id: &str) -> Arc<tokio::sync::Notify> {
    let mut guard = notifies().lock().unwrap_or_else(|e| e.into_inner());
    guard.entry(run_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Notify::new()))
        .clone()
}

const SESSION_RELEASE_SETTLE_MS: u64 = 2000;
const BASE32: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";

fn gen_run_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::getrandom(&mut bytes).expect("os rng");
    let mut s = String::from("r");
    for b in &bytes {
        s.push(BASE32[(b & 0x1f) as usize] as char);
    }
    s
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn ok_resp(extra: Value) -> String {
    let mut v = serde_json::json!({"ok": true});
    if let (Some(obj), Some(ext)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in ext {
            obj.insert(k.clone(), val.clone());
        }
    }
    v.to_string()
}

fn err_resp(code: &str, message: &str) -> String {
    serde_json::json!({"ok": false, "error": {"code": code, "message": message}}).to_string()
}

fn parse_body(body: &[u8]) -> Result<Value, String> {
    if body.is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_slice(body).map_err(|_| err_resp("bad_request", "invalid JSON body"))
}

fn err_from_str(e: &str) -> String {
    if let Some((code, msg)) = e.split_once(':') {
        err_resp(code, msg)
    } else {
        err_resp("bad_request", e)
    }
}

fn str_arr_field(v: &Value, key: &str) -> Vec<String> {
    v[key].as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn strict_str_arr(v: &Value, key: &str) -> Result<Vec<String>, String> {
    let Some(arr_val) = v.get(key).filter(|v| !v.is_null()) else {
        return Ok(vec![]);
    };
    let Some(arr) = arr_val.as_array() else {
        return Err(err_resp("bad_request", &format!("{key} must be an array")));
    };
    let mut result = Vec::new();
    for item in arr {
        match item.as_str() {
            Some(s) => result.push(s.to_string()),
            None => return Err(err_resp("bad_request", &format!("{key} elements must be strings"))),
        }
    }
    Ok(result)
}

enum CommitResult {
    Ok(Value),
    Stale,
    Err(String),
}

fn commit_sync<F>(make_cmd: F, run_id: &str, base: &std::path::Path) -> CommitResult
where
    F: FnOnce(&State) -> Result<Cmd, String>,
{
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());

    let cmd = match make_cmd(&guard) {
        Ok(c) => c,
        Err(e) => return CommitResult::Err(e),
    };

    let event_seq = guard.runs.get(run_id).map(|e| e.event_seq + 1).unwrap_or(1);
    let now = unix_ms();

    match guard.apply(&cmd) {
        ApplyResult::Ok(kind, data) => {
            if let Err(e) = ensure_run_dir(base, run_id) {
                return CommitResult::Err(format!("io:{e}"));
            }
            let path = run_events_path(base, run_id);
            let record = EventRecord { seq: event_seq, at: now, kind: kind.clone(), data: data.clone() };
            if let Err(e) = append_event(&path, &record) {
                return CommitResult::Err(format!("io:{e}"));
            }
            if let Err(e) = guard.replay(&kind, &data) {
                eprintln!("[orch] replay after commit {kind}: {e}");
            }
            if let Some(entry) = guard.runs.get_mut(run_id) {
                entry.event_seq = event_seq;
            }
            CommitResult::Ok(data)
        }
        ApplyResult::Stale(kind, data) => {
            let path = run_events_path(base, run_id);
            let record = EventRecord { seq: event_seq, at: now, kind: kind.clone(), data: data.clone() };
            if let Err(e) = append_event(&path, &record) {
                eprintln!("[orch] stale event append: {e}");
            } else if let Some(entry) = guard.runs.get_mut(run_id) {
                entry.event_seq = event_seq;
            }
            CommitResult::Stale
        }
        ApplyResult::Err(e) => CommitResult::Err(format!("{}:{}", e.code(), e.message())),
    }
}

async fn commit<F>(make_cmd: F, run_id: String) -> CommitResult
where
    F: FnOnce(&State) -> Result<Cmd, String> + Send + 'static,
{
    let Some(base) = crate::panorama_dir() else {
        return CommitResult::Err("io:no panorama dir".into());
    };
    let notify_id = run_id.clone();
    let result = tokio::task::spawn_blocking(move || commit_sync(make_cmd, &run_id, &base))
        .await
        .unwrap_or_else(|_| CommitResult::Err("io:spawn failed".into()));
    if !matches!(result, CommitResult::Err(_)) {
        run_notify(&notify_id).notify_waiters();
        crate::ui::broadcast_orch_changed(&notify_id);
    }
    result
}

fn collect_ack_sync(
    run_id: &str,
    recipient: &str,
    types: &Option<Vec<String>>,
    peek: bool,
    base: &std::path::Path,
) -> Result<Vec<Value>, String> {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = guard.runs.get(run_id) else {
        return Err("not_found:run not found".into());
    };

    let matched_seqs: Vec<u32> = entry.messages.iter()
        .filter(|m| !m.acked && m.to == recipient)
        .filter(|m| types.as_ref().map(|ts| ts.contains(&m.msg_type)).unwrap_or(true))
        .map(|m| m.seq)
        .collect();

    let msgs: Vec<Value> = entry.messages.iter()
        .filter(|m| matched_seqs.contains(&m.seq))
        .filter_map(|m| serde_json::to_value(m).ok())
        .collect();

    if peek || matched_seqs.is_empty() {
        return Ok(msgs);
    }

    let event_seq = entry.event_seq + 1;
    let ack_cmd = Cmd::Ack { run_id: run_id.to_string(), seqs: matched_seqs };

    let (kind, data) = match guard.apply(&ack_cmd) {
        ApplyResult::Ok(k, d) => (k, d),
        _ => return Ok(msgs),
    };

    if let Err(e) = ensure_run_dir(base, run_id) {
        return Err(format!("io:{e}"));
    }
    let path = run_events_path(base, run_id);
    let record = EventRecord { seq: event_seq, at: unix_ms(), kind: kind.clone(), data: data.clone() };
    if let Err(e) = append_event(&path, &record) {
        return Err(format!("io:{e}"));
    }
    if let Err(e) = guard.replay(&kind, &data) {
        eprintln!("[orch] replay ack: {e}");
    }
    if let Some(entry) = guard.runs.get_mut(run_id) {
        entry.event_seq = event_seq;
    }

    Ok(msgs)
}

pub fn init() {
    let Some(base) = crate::panorama_dir() else { return };

    let loaded = load_coordinators(&base);
    *coordinators().lock().unwrap_or_else(|e| e.into_inner()) = loaded;

    let runs_dir = base.join("orch").join("runs");
    if runs_dir.exists() {
        let Ok(entries) = std::fs::read_dir(&runs_dir) else {
            tokio::runtime::Handle::current().spawn(supervision_loop());
            return;
        };
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        for entry in entries.flatten() {
            let Some(run_id) = entry.file_name().to_str().map(str::to_string) else { continue };
            let path = entry.path().join("events.jsonl");
            load_into(&mut guard, &path, &run_id);
        }
    }
    tokio::runtime::Handle::current().spawn(supervision_loop());
}

pub async fn handle(command: &str, body: &[u8]) -> String {
    match command {
        "run-create" => handle_run_create(body).await,
        "run-update" => handle_run_update(body).await,
        "run-list" => handle_run_list(),
        "run-show" => handle_run_show(body),
        "run-set-status" => handle_run_set_status(body).await,
        "run-set-coordinator" => handle_run_set_coordinator(body).await,
        "task-create" => handle_task_create(body).await,
        "task-update" => handle_task_update(body).await,
        "task-list" => handle_task_list(body),
        "dispatch" => handle_dispatch(body).await,
        "send" => handle_send(body).await,
        "check" => handle_check(body).await,
        "gate-create" => handle_gate_create(body).await,
        "gate-resolve" => handle_gate_resolve(body).await,
        "worker-start" => handle_worker_start(body).await,
        "worker-show" => handle_worker_show(body),
        "worker-read" => handle_worker_read(body),
        "worker-send" => handle_worker_send(body),
        "task-cancel" => handle_task_cancel(body).await,
        "worker-stop" => handle_worker_stop(body).await,
        "worker-release" => handle_worker_release(body).await,
        "integrate" => handle_integrate(body).await,
        "worktree-clean" => handle_worktree_clean(body).await,
        "verify" => handle_verify(body).await,
        "retry" => handle_retry(body).await,
        "report" => handle_report(body).await,
        "settings-get" => handle_settings_get(),
        "settings-set" => handle_settings_set(body),
        "provider-save" => handle_provider_save(body),
        "provider-delete" => handle_provider_delete(body),
        "provider-test" => handle_provider_test(body).await,
        "agent-list" => handle_agent_list(body).await,
        "coordinator-set" => handle_coordinator_set(body).await,
        "coordinator-list" => handle_coordinator_list(),
        "dispatch-adopt" => handle_dispatch_adopt(body).await,
        "ai-commit-message" => handle_ai_commit_message(body).await,
        "ai-tidy" => handle_ai_tidy(body).await,
        _ => err_resp("bad_request", "unknown command"),
    }
}

pub fn on_agent_idle(tile_id: &str) {
    let cli = crate::panorama_cli_path();
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    for (run_id, entry) in &guard.runs {
        if entry.run.status.is_terminal() { continue; }
        if let Some(ref coord_tile) = entry.run.coordinator_tile {
            if coord_tile == tile_id {
                let unacked: usize = entry.messages.iter()
                    .filter(|m| !m.acked && m.to == "coordinator")
                    .count();
                if unacked > 0 {
                    let run_id = run_id.clone();
                    let tile = tile_id.to_string();
                    let cli2 = cli.clone();
                    drop(guard);
                    maybe_nudge_tile(&tile, &run_id, unacked, &cli2);
                    return;
                }
            }
        }
        for dispatch in entry.dispatches.values() {
            if let Some(ref dtile) = dispatch.tile_id {
                if dtile == tile_id && dispatch.accepts_reports() {
                    let unacked: usize = entry.messages.iter()
                        .filter(|m| !m.acked && m.to.as_str() == dispatch.id.as_str())
                        .count();
                    if unacked > 0 {
                        let tile = tile_id.to_string();
                        let cli2 = cli.clone();
                        drop(guard);
                        maybe_nudge_worker(&tile, &cli2);
                        return;
                    }
                }
            }
        }
    }
}

pub fn on_agent_waiting(tile_id: &str) {
    let now = unix_ms();
    let cli = crate::panorama_cli_path();
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    for (run_id, entry) in &guard.runs {
        if entry.run.status.is_terminal() { continue; }
        for dispatch in entry.dispatches.values() {
            if let Some(ref dtile) = dispatch.tile_id {
                if dtile == tile_id && dispatch.accepts_reports() {
                    let run_id = run_id.clone();
                    let dispatch_id = dispatch.id.clone();
                    drop(guard);
                    tokio::spawn(async move {
                        let run_id2 = run_id.clone();
                        commit(move |g| {
                            let entry = g.runs.get(&run_id2)
                                .ok_or_else(|| "not_found:run not found".to_string())?;
                            let seq = entry.msg_seq + 1;
                            Ok(Cmd::Send {
                                run_id: run_id2.clone(),
                                seq,
                                from: dispatch_id.clone(),
                                to: vec!["coordinator".into()],
                                msg_type: "status".into(),
                                task_id: None,
                                dispatch_id: Some(dispatch_id.clone()),
                                reply_to: None,
                                body: Some("worker waiting for permission".into()),
                                outcome: None,
                                files_modified: vec![],
                                summary: None,
                                now,
                            })
                        }, run_id.clone()).await;
                        if let Some(coord_tile) = {
                            let g = state().lock().unwrap_or_else(|e| e.into_inner());
                            g.runs.get(&run_id).and_then(|e| e.run.coordinator_tile.clone())
                        } {
                            if !is_tile_waiting(&coord_tile) {
                                maybe_nudge_tile(&coord_tile, &run_id, 1, &cli);
                            }
                        }
                    });
                    return;
                }
            }
        }
    }
}

fn parse_limits(v: &Value) -> Result<Limits, String> {
    let get_u64 = |flat: &str| -> Option<u64> {
        v.get("limits")
            .and_then(|l| l.get(flat))
            .or_else(|| v.get(flat))
            .and_then(|n| n.as_u64())
    };
    let mw = get_u64("maxWorkers").unwrap_or(4);
    let ma = get_u64("maxAttempts").unwrap_or(3);
    let dt = get_u64("dispatchTimeoutMin").unwrap_or(60);
    let vt = get_u64("verifyTimeoutMin").unwrap_or(15);
    let sq = get_u64("stallQuietMin").unwrap_or(5);
    if mw < 1 { return Err(err_resp("bad_request", "maxWorkers must be >= 1")); }
    if ma < 1 { return Err(err_resp("bad_request", "maxAttempts must be >= 1")); }
    if dt < 1 { return Err(err_resp("bad_request", "dispatchTimeoutMin must be >= 1")); }
    Ok(Limits {
        max_workers: mw as u32,
        max_attempts: ma as u32,
        dispatch_timeout_min: dt as u32,
        verify_timeout_min: vt as u32,
        stall_quiet_min: sq as u32,
    })
}

async fn handle_run_create(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let repo = v["repo"].as_str().unwrap_or("").to_string();
    if repo.is_empty() { return err_resp("bad_request", "repo required"); }
    if !std::path::Path::new(&repo).is_dir() {
        return err_resp("bad_request", "repo is not an existing directory");
    }
    let limits = match parse_limits(&v) { Ok(l) => l, Err(e) => return e };
    let id = gen_run_id();
    let title = v["title"].as_str().unwrap_or("untitled").to_string();
    let base_ref = v["baseRef"].as_str().map(str::to_string);
    let setup = v["setup"].as_str().map(str::to_string);
    let coordinator_tile = v["coordinatorTile"].as_str().map(str::to_string);

    let rev = base_ref.clone().unwrap_or_else(|| "HEAD".into());
    let repo_c = repo.clone();
    let base_sha = tokio::task::spawn_blocking(move || {
        crate::orch::git::rev_parse(&repo_c, &rev).ok()
    }).await.unwrap_or(None);

    let id_c = id.clone();
    let now = unix_ms();
    let result = commit(move |_| {
        Ok(Cmd::RunCreate { id: id_c, title, repo, base_ref, base_sha, setup, coordinator_tile, limits, now })
    }, id.clone()).await;
    match result {
        CommitResult::Ok(_) => {
            let (run_val, coord_tile) = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                let entry = guard.runs.get(&id);
                let run = entry.map(|e| serde_json::to_value(&e.run).unwrap_or_default());
                let tile = entry.and_then(|e| e.run.coordinator_tile.clone());
                (run, tile)
            };
            if let Some(ref tile) = coord_tile {
                let cost_usd = crate::tile_cost_usd(tile);
                let run_id_b = id.clone();
                let now_b = unix_ms();
                let _ = commit(move |_| {
                    Ok(Cmd::RunRecordCoordinatorBaseline { run_id: run_id_b, cost_usd, now: now_b })
                }, id.clone()).await;
            }
            ok_resp(serde_json::json!({"run": run_val}))
        }
        CommitResult::Stale => err_resp("stale", "run create stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

struct CostEntry {
    usd: f64,
    dispatches: usize,
    reported: usize,
}

fn cost_aggregates(entry: &reducer::RunEntry) -> (Value, Value) {
    let mut by_role: HashMap<String, CostEntry> = HashMap::new();
    let mut by_model: HashMap<String, CostEntry> = HashMap::new();

    for d in entry.dispatches.values() {
        let task_role = entry.tasks.get(d.task_id.as_str())
            .and_then(|t| t.role.as_ref())
            .map(|r| r.as_str().to_string())
            .unwrap_or_else(|| "implement".to_string());

        let model_key = match (d.profile.as_deref(), d.model.as_deref()) {
            (Some(p), Some(m)) => format!("{p}:{m}"),
            (Some(p), None) => p.to_string(),
            _ => "unknown".to_string(),
        };

        let has_cost = d.cost_usd.is_some();
        let cost_val = d.cost_usd.unwrap_or(0.0);

        let re = by_role.entry(task_role).or_insert(CostEntry { usd: 0.0, dispatches: 0, reported: 0 });
        re.dispatches += 1;
        if has_cost { re.usd += cost_val; re.reported += 1; }

        let me = by_model.entry(model_key).or_insert(CostEntry { usd: 0.0, dispatches: 0, reported: 0 });
        me.dispatches += 1;
        if has_cost { me.usd += cost_val; me.reported += 1; }
    }

    let role_json: serde_json::Map<String, Value> = by_role.into_iter()
        .map(|(k, v)| (k, serde_json::json!({"usd": v.usd, "dispatches": v.dispatches, "reported": v.reported})))
        .collect();
    let model_json: serde_json::Map<String, Value> = by_model.into_iter()
        .map(|(k, v)| (k, serde_json::json!({"usd": v.usd, "dispatches": v.dispatches, "reported": v.reported})))
        .collect();

    (Value::Object(role_json), Value::Object(model_json))
}

fn coordinator_cost_usd(entry: &reducer::RunEntry) -> Option<f64> {
    if let Some(frozen) = entry.coordinator_cost_final {
        return Some(frozen);
    }
    let tile_id = entry.run.coordinator_tile.as_deref()?;
    let baseline = entry.coordinator_cost_baseline?;
    let current = crate::tile_cost_usd(tile_id)?;
    Some((current - baseline).max(0.0))
}

fn handle_run_list() -> String {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let runs: Vec<Value> = guard.runs.values().map(|e| {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for t in e.tasks.values() {
            *counts.entry(format!("{:?}", t.status).to_lowercase()).or_insert(0) += 1;
        }
        let open_gates = e.gates.values().filter(|g| g.resolution.is_none()).count();
        let (cost_by_role, cost_by_model) = cost_aggregates(e);
        let coord_cost = coordinator_cost_usd(e);
        let mut row = serde_json::json!({"run": e.run, "taskCounts": counts, "openGates": open_gates,
            "costByRole": cost_by_role, "costByModel": cost_by_model});
        if let Some(c) = coord_cost {
            row["coordinatorCostUsd"] = c.into();
        }
        row
    }).collect();
    ok_resp(serde_json::json!({"runs": runs}))
}

fn handle_run_show(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let brief_mode = v["brief"].as_bool().unwrap_or(false);

    struct DispatchRow {
        val: Value,
        tile_id: Option<String>,
        dispatch_id: String,
        is_running: bool,
    }

    let (run_val, tasks, dispatch_rows, gates, cost_by_role, cost_by_model, coord_cost) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(&run_id) else {
            return err_resp("not_found", "run not found");
        };
        let tasks: Vec<Value> = entry.tasks.values().map(|t| {
            let ready = entry.task_ready(t);
            let mut tv = serde_json::to_value(t).unwrap_or_default();
            if let Some(obj) = tv.as_object_mut() {
                obj.insert("ready".into(), ready.into());
            }
            tv
        }).collect();
        let dispatch_rows: Vec<DispatchRow> = entry.dispatches.values().map(|d| {
            DispatchRow {
                val: serde_json::to_value(d).unwrap_or_default(),
                tile_id: d.tile_id.clone(),
                dispatch_id: d.id.clone(),
                is_running: d.status == DispatchStatus::Running,
            }
        }).collect();
        let gates: Vec<Value> = entry.gates.values()
            .map(|g| serde_json::to_value(g).unwrap_or_default())
            .collect();
        let run_val = serde_json::to_value(&entry.run).unwrap_or_default();
        let (cost_by_role, cost_by_model) = cost_aggregates(entry);
        let coord_cost = coordinator_cost_usd(entry);
        (run_val, tasks, dispatch_rows, gates, cost_by_role, cost_by_model, coord_cost)
    };

    let starting_set = starting().lock().unwrap_or_else(|e| e.into_inner()).clone();
    let dispatches: Vec<Value> = dispatch_rows.into_iter().map(|row| {
        let mut val = row.val;
        let live = row.tile_id.as_deref().map(|t| crate::lookup_live(t).is_some()).unwrap_or(false);
        let agent_status = row.tile_id.as_deref()
            .and_then(|t| crate::sessions().lock().unwrap().get(t).cloned())
            .and_then(|s| crate::cached_agent_state(&s))
            .and_then(|js| serde_json::from_str::<Value>(&js).ok())
            .and_then(|v| v["lastStatus"].as_str().map(str::to_string));
        let is_starting = starting_set.contains(&row.dispatch_id);
        if let Some(obj) = val.as_object_mut() {
            if row.is_running {
                let waiting = row.tile_id.as_deref().map(is_tile_waiting).unwrap_or(false);
                obj.insert("waiting".into(), waiting.into());
            }
            obj.insert("live".into(), live.into());
            obj.insert("starting".into(), is_starting.into());
            if let Some(s) = agent_status {
                obj.insert("agentStatus".into(), s.into());
            }
        }
        val
    }).collect();

    if brief_mode {
        let dispatches_brief: Vec<Value> = dispatches.iter().map(|d| {
            serde_json::json!({
                "id": d["id"],
                "taskId": d["taskId"],
                "attempt": d["attempt"],
                "status": d["status"],
                "live": d["live"],
                "agentStatus": d.get("agentStatus"),
                "starting": d["starting"],
                "summary": d["summary"].as_str().and_then(|s| s.lines().next()).unwrap_or("")
            })
        }).collect();
        let tasks_brief: Vec<Value> = tasks.iter().map(|t| {
            serde_json::json!({
                "id": t["id"],
                "title": t["title"],
                "status": t["status"],
                "owner": t["owner"],
                "attempts": t["attempts"],
                "ready": t["ready"]
            })
        }).collect();
        let gates_brief: Vec<Value> = gates.iter()
            .filter(|g| g["resolution"].is_null())
            .map(|g| serde_json::json!({"id": g["id"], "taskId": g["taskId"], "question": g["question"]}))
            .collect();
        return ok_resp(serde_json::json!({
            "run": {"id": run_val["id"], "title": run_val["title"], "status": run_val["status"]},
            "tasks": tasks_brief,
            "dispatches": dispatches_brief,
            "gates": gates_brief,
        }));
    }

    let _ = dispatches.len();
    let mut resp = serde_json::json!({
        "run": run_val,
        "tasks": tasks,
        "dispatches": dispatches,
        "gates": gates,
        "costByRole": cost_by_role,
        "costByModel": cost_by_model,
    });
    if let Some(c) = coord_cost {
        resp["coordinatorCostUsd"] = c.into();
    }
    ok_resp(resp)
}

pub fn on_agent_resumed(tile_id: &str) {
    let has_dispatch = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.values().any(|e| e.dispatches.values()
            .any(|d| d.status == DispatchStatus::Running && d.tile_id.as_deref() == Some(tile_id)))
    };
    if has_dispatch {
        crate::ui::broadcast_orch_changed("");
    }
}

async fn handle_run_set_status(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let status: RunStatus = match v.get("status").filter(|s| !s.is_null()) {
        None => return err_resp("bad_request", "valid status required"),
        Some(s) => match serde_json::from_value(s.clone()) {
            Ok(ts) => ts,
            Err(_) => return err_resp("bad_request", "invalid status value"),
        }
    };
    let is_terminal = matches!(status, RunStatus::Completed | RunStatus::Cancelled);
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::RunSetStatus { run_id: run_id_c, status, now })
    }, run_id.clone()).await;
    match result {
        CommitResult::Ok(_) => {
            if is_terminal {
                let (tile_id, baseline) = {
                    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                    let entry = guard.runs.get(&run_id);
                    let tile = entry.and_then(|e| e.run.coordinator_tile.clone());
                    let base = entry.and_then(|e| e.coordinator_cost_baseline);
                    (tile, base)
                };
                if let (Some(ref tile), Some(bl)) = (tile_id, baseline) {
                    if let Some(current) = crate::tile_cost_usd(tile) {
                        let delta = (current - bl).max(0.0);
                        let run_id_b = run_id.clone();
                        let now_b = unix_ms();
                        let _ = commit(move |_| {
                            Ok(Cmd::RunCoordinatorCost { run_id: run_id_b, cost_usd: delta, now: now_b })
                        }, run_id).await;
                    }
                }
            }
            ok_resp(serde_json::json!({}))
        }
        CommitResult::Stale => err_resp("stale", "run set-status stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_run_update(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let title = v["title"].as_str().unwrap_or("").to_string();
    if title.is_empty() { return err_resp("bad_request", "title required"); }
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::RunUpdate { run_id: run_id_c, title, now })
    }, run_id).await;
    match result {
        CommitResult::Ok(_) => ok_resp(serde_json::json!({})),
        CommitResult::Stale => err_resp("stale", "run update stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_run_set_coordinator(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let tile_id: Option<String> = v["tileId"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let now = unix_ms();
    let tile_id_c = tile_id.clone();
    let run_id_c = run_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::RunSetCoordinator { run_id: run_id_c, tile_id: tile_id_c, now })
    }, run_id.clone()).await;
    match result {
        CommitResult::Ok(_) => {
            if let Some(ref tid) = tile_id {
                let snapshot = {
                    let mut guard = coordinators().lock().unwrap_or_else(|e| e.into_inner());
                    guard.insert(tid.clone());
                    guard.clone()
                };
                if let Err(e) = save_coordinators(&base, &snapshot) {
                    return err_resp("io", &e);
                }
                let cost_usd = crate::tile_cost_usd(tid);
                let run_id_b = run_id.clone();
                let now_b = unix_ms();
                let _ = commit(move |_| {
                    Ok(Cmd::RunRecordCoordinatorBaseline { run_id: run_id_b, cost_usd, now: now_b })
                }, run_id).await;
            }
            ok_resp(serde_json::json!({}))
        }
        CommitResult::Stale => err_resp("stale", "run set-coordinator stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_task_create(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let title = v["title"].as_str().unwrap_or("untitled").to_string();
    let spec = v["spec"].as_str().unwrap_or("").to_string();
    let acceptance = v["acceptance"].as_str().map(str::to_string);
    let profile = v["profile"].as_str().map(str::to_string);
    let review = v["review"].as_bool().unwrap_or(false);
    let owner: Option<String> = v["owner"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let by = v["by"].as_str().unwrap_or("coordinator").to_string();
    let now = unix_ms();

    let deps = match strict_str_arr(&v, "deps") { Ok(a) => a, Err(e) => return e };
    let mut escalation = match strict_str_arr(&v, "escalation") { Ok(a) => a, Err(e) => return e };
    let verify = match strict_str_arr(&v, "verify") { Ok(a) => a, Err(e) => return e };

    let role: Option<model::Role> = match v.get("role").filter(|r| !r.is_null()) {
        None => None,
        Some(r) => match serde_json::from_value::<model::Role>(r.clone()) {
            Ok(role) => Some(role),
            Err(_) => return err_resp("bad_role", &format!(
                "role must be one of: {}; got: {}",
                model::Role::all().join(", "),
                r.as_str().unwrap_or("(invalid)")
            )),
        },
    };

    if escalation.is_empty() && profile.is_none() {
        let Some(base) = crate::panorama_dir() else {
            return err_resp("io", "no panorama dir");
        };
        let s = settings::load(&base);
        let effective_role = role.as_ref().map(|r| r.as_str()).unwrap_or("implement");
        let chain = match effective_role {
            "explore" => &s.roles.explore,
            "implement" => &s.roles.implement,
            "test" => &s.roles.test,
            "review" => &s.roles.review,
            "mechanical" => &s.roles.mechanical,
            _ => &s.roles.implement,
        };
        if chain.is_empty() {
            return err_resp("empty_role_chain", &format!(
                "role chain for '{effective_role}' is empty; set it in Settings > Orchestration or pass --escalation"
            ));
        }
        escalation = chain.clone();
    }

    let owner_c = owner.clone();
    let run_id_c = run_id.clone();
    let title_c = title.clone();
    let role_c = role.clone();
    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let task_id = entry.next_task_id();
        Ok(Cmd::TaskCreate {
            run_id: run_id_c, id: task_id,
            title: title_c, spec, acceptance, deps, profile, escalation, verify, role: role_c,
            review, owner: owner_c, now,
        })
    }, run_id.clone()).await;

    match result {
        CommitResult::Ok(data) => {
            let task_id = data["id"].as_str().unwrap_or("").to_string();
            let (task_val, coord_tile) = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                let entry = guard.runs.get(&run_id);
                let task = entry.and_then(|e| e.tasks.get(&task_id))
                    .map(|t| serde_json::to_value(t).unwrap_or_default());
                let coord = entry.and_then(|e| e.run.coordinator_tile.clone());
                (task, coord)
            };
            if by == "user" && coord_tile.is_some() {
                let body_str = format!("user added task {task_id}: {title}");
                if let Err(e) = notify_coordinator_from_user(&run_id, &task_id, body_str).await {
                    return err_resp("io", &format!("task created, but the coordinator was not notified: {e}"));
                }
            }
            ok_resp(serde_json::json!({"task": task_val}))
        }
        CommitResult::Stale => err_resp("stale", "task create stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_task_update(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let status: Option<TaskStatus> = match v.get("status").filter(|s| !s.is_null()) {
        None => None,
        Some(s) => match serde_json::from_value(s.clone()) {
            Ok(ts) => Some(ts),
            Err(_) => return err_resp("bad_request", "invalid status value"),
        }
    };
    let verify: Option<Vec<String>> = match v.get("verify").filter(|s| !s.is_null()) {
        None => None,
        Some(s) => match serde_json::from_value(s.clone()) {
            Ok(ts) => Some(ts),
            Err(_) => return err_resp("bad_request", "verify must be array of strings"),
        }
    };
    let escalation: Option<Vec<String>> = match v.get("escalation").filter(|s| !s.is_null()) {
        None => None,
        Some(s) => match serde_json::from_value(s.clone()) {
            Ok(ts) => Some(ts),
            Err(_) => return err_resp("bad_request", "escalation must be array of strings"),
        }
    };
    let result_val = v["result"].as_str().map(str::to_string);
    let spec = v["spec"].as_str().map(str::to_string);
    let acceptance = v["acceptance"].as_str().map(str::to_string);
    let profile = v["profile"].as_str().map(str::to_string);
    let review_field: Option<bool> = v.get("review").filter(|s| !s.is_null()).and_then(|s| s.as_bool());
    let by = v["by"].as_str().unwrap_or("coordinator").to_string();
    let now = unix_ms();

    let prev_status = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.tasks.get(&task_id))
            .map(|t| t.status.clone())
    };

    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let commit_result = commit(move |_| {
        Ok(Cmd::TaskUpdate {
            run_id: run_id_c, task_id: task_id_c, status, result: result_val,
            spec, acceptance, verify, profile, escalation, review: review_field, now,
        })
    }, run_id.clone()).await;

    match commit_result {
        CommitResult::Ok(_) => {
            let (new_status, coord_tile) = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                let entry = guard.runs.get(&run_id);
                let status = entry.and_then(|e| e.tasks.get(&task_id)).map(|t| t.status.clone());
                (status, entry.and_then(|e| e.run.coordinator_tile.clone()))
            };
            if by != "user" || coord_tile.is_none() {
                return ok_resp(serde_json::json!({}));
            }
            let body_str = match (&prev_status, &new_status) {
                (Some(TaskStatus::Review), Some(TaskStatus::Completed)) => format!("user accepted {task_id}"),
                (Some(TaskStatus::Review), Some(TaskStatus::Pending)) if v["spec"].is_string() => {
                    format!("user reopened {task_id}; see the task spec for the reviewer note")
                }
                (Some(TaskStatus::Review), Some(TaskStatus::Pending)) => format!("user reopened {task_id}"),
                _ => return ok_resp(serde_json::json!({})),
            };
            if let Err(e) = notify_coordinator_from_user(&run_id, &task_id, body_str).await {
                return err_resp("io", &format!("task updated, but the coordinator was not notified: {e}"));
            }
            ok_resp(serde_json::json!({}))
        }
        CommitResult::Stale => err_resp("stale", "task update stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

fn handle_task_list(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let filter: Option<TaskStatus> = match v.get("status").filter(|s| !s.is_null()) {
        None => None,
        Some(s) => match serde_json::from_value(s.clone()) {
            Ok(ts) => Some(ts),
            Err(_) => return err_resp("bad_request", "invalid status filter"),
        }
    };
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = guard.runs.get(&run_id) else {
        return err_resp("not_found", "run not found");
    };
    let tasks: Vec<Value> = entry.tasks.values()
        .filter(|t| filter.as_ref().map(|f| &t.status == f).unwrap_or(true))
        .map(|t| serde_json::to_value(t).unwrap_or_default())
        .collect();
    ok_resp(serde_json::json!({"tasks": tasks}))
}

async fn handle_dispatch(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let retry_of = v["retryOf"].as_str().map(str::to_string);
    let profile = v["profile"].as_str().map(str::to_string);
    let model = v["model"].as_str().map(str::to_string);
    let tile_id = v["tileId"].as_str().map(str::to_string);
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let task = entry.tasks.get(&task_id_c)
            .ok_or_else(|| "not_found:task not found".to_string())?;
        let attempt = task.attempts + 1;
        let dispatch_id = format!("{run_id_c}.{task_id_c}.{attempt}");
        Ok(Cmd::Dispatch {
            run_id: run_id_c, id: dispatch_id, task_id: task_id_c,
            attempt, retry_of, profile, model, tile_id, owner: None, now,
        })
    }, run_id.clone()).await;
    match result {
        CommitResult::Ok(data) => {
            let dispatch_id = data["id"].as_str().unwrap_or("").to_string();
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let dispatch = guard.runs.get(&run_id)
                .and_then(|e| e.dispatches.get(&dispatch_id))
                .map(|d| serde_json::to_value(d).unwrap_or_default());
            ok_resp(serde_json::json!({"dispatch": dispatch}))
        }
        CommitResult::Stale => err_resp("stale", "dispatch stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_send(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let msg_type = v["type"].as_str().unwrap_or("").to_string();
    if msg_type.is_empty() { return err_resp("bad_request", "type required"); }
    let from = v["from"].as_str().unwrap_or("coordinator").to_string();
    let to_raw = v["to"].clone();
    let files_modified = match strict_str_arr(&v, "filesModified") {
        Ok(a) => a,
        Err(e) => return e,
    };
    let task_id = v["taskId"].as_str().map(str::to_string);
    let dispatch_id = v["dispatchId"].as_str().map(str::to_string);
    let dispatch_id_for_hook = dispatch_id.clone();
    let reply_to = v["replyTo"].as_u64().map(|n| n as u32);
    let body_str = v["body"].as_str().map(str::to_string);
    let outcome = v["outcome"].as_str().map(str::to_string);
    let summary = v["summary"].as_str().map(str::to_string);
    if msg_type == "worker_done" {
        if let Some(did) = dispatch_id.as_deref() {
            commit_worker_leftovers(&run_id, did).await;
        }
    }
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let msg_type_c = msg_type.clone();
    let from_c = from.clone();
    let files_modified_c = files_modified.clone();

    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let to: Vec<String> = if to_raw.as_str() == Some("@workers") {
            entry.dispatches.values()
                .filter(|d| d.status == DispatchStatus::Running)
                .map(|d| d.id.clone())
                .collect()
        } else if let Some(arr) = to_raw.as_array() {
            arr.iter().filter_map(|x| x.as_str()).map(str::to_string).collect()
        } else if let Some(s) = to_raw.as_str() {
            vec![s.to_string()]
        } else {
            return Err("bad_request:to required".into());
        };
        let seq = entry.next_msg_seq();
        Ok(Cmd::Send {
            run_id: run_id_c, seq, from: from_c, to,
            msg_type: msg_type_c, task_id, dispatch_id, reply_to,
            body: body_str, outcome, files_modified: files_modified_c, summary, now,
        })
    }, run_id.clone()).await;

    match result {
        CommitResult::Ok(_) => {
            if msg_type == "worker_done" {
                if let Some(did) = dispatch_id_for_hook {
                    let run_id_wd = run_id.clone();
                    tokio::spawn(async move {
                        worker_done_side_effects(run_id_wd, did).await;
                    });
                }
            }
            ok_resp(serde_json::json!({}))
        }
        CommitResult::Stale => {
            if msg_type == "worker_done" {
                if let Some(did) = dispatch_id_for_hook {
                    let run_id_lr = run_id.clone();
                    let outcome_lr = v["outcome"].as_str().unwrap_or("").to_string();
                    let summary_lr = v["summary"].as_str().unwrap_or("").to_string();
                    let files_lr = files_modified.clone();
                    tokio::spawn(async move {
                        let now_lr = unix_ms();
                        let body_lr = serde_json::json!({
                            "dispatchId": did,
                            "outcome": outcome_lr,
                            "summary": summary_lr,
                            "filesModified": files_lr,
                            "reason": "superseded"
                        }).to_string();
                        let run_id_lr2 = run_id_lr.clone();
                        let did2 = did.clone();
                        commit(move |g| {
                            let entry = g.runs.get(&run_id_lr2)
                                .ok_or_else(|| "not_found:run not found".to_string())?;
                            let seq = entry.next_msg_seq();
                            Ok(Cmd::Send {
                                run_id: run_id_lr2,
                                seq,
                                from: "brain".into(),
                                to: vec!["coordinator".into()],
                                msg_type: "late_report".into(),
                                task_id: None,
                                dispatch_id: Some(did2),
                                reply_to: None,
                                body: Some(body_lr),
                                outcome: None,
                                files_modified: vec![],
                                summary: None,
                                now: now_lr,
                            })
                        }, run_id_lr).await;
                    });
                }
                return err_resp("stale", "this dispatch was superseded by a newer attempt; report not applied, the coordinator was told");
            }
            err_resp("stale", "report from a superseded dispatch")
        }
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_dispatch_adopt(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || dispatch_id.is_empty() {
        return err_resp("bad_request", "runId and dispatchId required");
    }

    let (task_id, accepts) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let d = match entry.dispatches.get(&dispatch_id) {
            Some(d) => d,
            None => return err_resp("not_found", "dispatch not found"),
        };
        (d.task_id.clone(), d.accepts_reports())
    };

    if !accepts {
        return err_resp("conflict", "dispatch does not accept reports");
    }

    let summary = match v["summary"].as_str() {
        Some(s) => format!("adopted by coordinator: {s}"),
        None => "adopted by coordinator".to_string(),
    };

    let adopt_body = serde_json::json!({
        "runId": run_id,
        "type": "worker_done",
        "from": dispatch_id,
        "to": ["coordinator"],
        "outcome": "succeeded",
        "summary": summary,
        "taskId": task_id,
        "dispatchId": dispatch_id,
        "filesModified": [],
    });
    handle_send(adopt_body.to_string().as_bytes()).await
}

// Runs before worker_done is recorded, so a coordinator that integrates or cleans as soon as it
// reads the message always sees the worker's changes committed on the task branch.
async fn commit_worker_leftovers(run_id: &str, dispatch_id: &str) {
    let target = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(run_id).and_then(|entry| {
            let d = entry.dispatches.get(dispatch_id)?;
            if !d.accepts_reports() {
                return None;
            }
            let title = entry.tasks.get(&d.task_id).map(|t| t.title.clone()).unwrap_or_default();
            Some((d.worktree.clone()?, format!("orch({}): {title}", d.task_id)))
        })
    };
    let Some((wt, message)) = target else { return };
    let _ = tokio::task::spawn_blocking(move || {
        let dirty = crate::orch::git::status_porcelain(&wt).map(|s| !s.trim().is_empty()).unwrap_or(false);
        if dirty {
            if let Err(e) = crate::orch::git::commit_leftovers(&wt, &message) {
                eprintln!("[orch] commit_leftovers failed: {e}");
            }
        }
    }).await;
}

async fn worker_done_side_effects(run_id: String, dispatch_id: String) {
    let (wt_path, base_sha, commit_msg, task_id, tile_id, dispatch_profile) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return,
        };
        let d = match entry.dispatches.get(&dispatch_id) {
            Some(d) => d,
            None => return,
        };
        let wt = match d.worktree.clone() {
            Some(p) => p,
            None => return,
        };
        let task_id = d.task_id.clone();
        let tile_id = d.tile_id.clone();
        let dispatch_profile = d.profile.clone();
        let title = entry.tasks.get(&task_id).map(|t| t.title.clone()).unwrap_or_default();
        (wt, entry.run.base_sha.clone(), format!("orch({task_id}): {title}"), task_id, tile_id, dispatch_profile)
    };

    let wt = wt_path.clone();
    let base = base_sha.clone();
    let result = tokio::task::spawn_blocking(move || -> (Option<String>, Vec<String>) {
        let status = crate::orch::git::status_porcelain(&wt).unwrap_or_default();
        if !status.trim().is_empty() {
            if let Err(e) = crate::orch::git::commit_leftovers(&wt, &commit_msg) {
                eprintln!("[orch] commit_leftovers failed: {e}");
            }
        }
        let sha = crate::orch::git::head_sha(&wt).ok();
        let files = base
            .as_deref()
            .and_then(|b| crate::orch::git::diff_name_only(&wt, b).ok())
            .unwrap_or_default();
        (sha, files)
    }).await;

    let head_sha = match result {
        Ok((Some(sha), files)) => {
            let run_id_c = run_id.clone();
            let dispatch_id_c = dispatch_id.clone();
            let now = unix_ms();
            let sha_c = sha.clone();
            commit(move |_| {
                Ok(Cmd::DispatchRecordHead {
                    run_id: run_id_c,
                    dispatch_id: dispatch_id_c,
                    head_sha: sha_c,
                    files_modified: files,
                    now,
                })
            }, run_id.clone()).await;
            Some(sha)
        }
        Ok((None, _)) => {
            eprintln!("[orch] worker_done_side_effects: no head sha for {dispatch_id}");
            None
        }
        Err(e) => {
            eprintln!("[orch] worker_done_side_effects spawn_blocking panic: {e}");
            None
        }
    };

    let is_provider_profile = dispatch_profile
        .as_deref()
        .and_then(|pid| {
            crate::panorama_dir().map(|base| {
                let profiles = load_profiles(&base);
                find_profile(&profiles, pid).map(|p| p.provider).unwrap_or(false)
            })
        })
        .unwrap_or(false);

    let (cost_usd, cost_source) = if is_provider_profile {
        (None, "provider".to_string())
    } else {
        tile_id
            .as_deref()
            .and_then(|tid| crate::sessions().lock().unwrap().get(tid).cloned())
            .and_then(|s| crate::cached_agent_state(&s))
            .and_then(|js| serde_json::from_str::<serde_json::Value>(&js).ok())
            .and_then(|v| v["costUsd"].as_f64())
            .map(|c| (Some(c), "exact".to_string()))
            .unwrap_or((None, "unknown".to_string()))
    };

    let run_id_c = run_id.clone();
    let dispatch_id_c = dispatch_id.clone();
    let now = unix_ms();
    commit(move |_| {
        Ok(Cmd::DispatchRecordCost {
            run_id: run_id_c,
            dispatch_id: dispatch_id_c,
            cost_usd,
            cost_source,
            now,
        })
    }, run_id.clone()).await;

    let needs_verify = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.tasks.get(&task_id))
            .map(|t| t.status == TaskStatus::Verifying && !t.verify.is_empty())
            .unwrap_or(false)
    };
    if needs_verify {
        let run_id2 = run_id.clone();
        let task_id2 = task_id.clone();
        let dispatch_id2 = dispatch_id.clone();
        let head = head_sha.clone();
        tokio::spawn(async move {
            run_verification(run_id2, task_id2, dispatch_id2, head).await;
        });
    }
}

struct EscalationInput<'a> {
    escalation: &'a [String],
    attempt: u32,
    explicit_profile: Option<&'a str>,
    explicit_model: Option<&'a str>,
    task_profile: Option<&'a str>,
    latest_profile: Option<&'a str>,
    latest_model: Option<&'a str>,
}

fn pick_escalation(input: EscalationInput) -> (String, Option<String>) {
    let EscalationInput {
        escalation, attempt, explicit_profile, explicit_model, task_profile, latest_profile, latest_model,
    } = input;
    if let Some(ep) = explicit_profile {
        (ep.to_string(), explicit_model.map(str::to_string))
    } else if !escalation.is_empty() {
        let idx = ((attempt as usize).saturating_sub(1)).min(escalation.len() - 1);
        let entry = &escalation[idx];
        let (p, m) = entry.split_once(':')
            .map(|(p, m)| (p.to_string(), Some(m.to_string())))
            .unwrap_or_else(|| (entry.clone(), None));
        let m = explicit_model.map(str::to_string).or(m);
        (p, m)
    } else {
        let raw = task_profile.or(latest_profile).unwrap_or("");
        let (p, task_m) = raw.split_once(':')
            .map(|(p, m)| (p.to_string(), Some(m.to_string())))
            .unwrap_or_else(|| (raw.to_string(), None));
        let m = explicit_model.map(str::to_string).or(task_m).or_else(|| latest_model.map(str::to_string));
        (p, m)
    }
}

async fn run_verification(run_id: String, task_id: String, dispatch_id: String, head_sha: Option<String>) {
    use crate::orch::exec::{RunToExitInput, run_to_exit};

    let (verify_cmds, worktree, verify_timeout_min, run_repo) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return,
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return,
        };
        let d = match entry.dispatches.get(&dispatch_id) {
            Some(d) => d,
            None => return,
        };
        let cwd = d.worktree.clone().unwrap_or_else(|| entry.run.repo.clone());
        (task.verify.clone(), cwd, entry.run.limits.verify_timeout_min, entry.run.repo.clone())
    };
    let _ = run_repo;

    for cmd in &verify_cmds {
        let input = RunToExitInput {
            cwd: worktree.clone(),
            cmd: cmd.clone(),
            timeout: std::time::Duration::from_secs(verify_timeout_min as u64 * 60),
            dispatch_id: dispatch_id.clone(),
            purpose: "verify".into(),
        };
        let outcome = match run_to_exit(input).await {
            Ok(o) => o,
            Err(e) => {
                eprintln!("[orch] run_verification start error for {dispatch_id}: {e}");
                return;
            }
        };

        let tail: Vec<String> = outcome.tail.iter().rev().take(40).rev().cloned().collect();
        let now = unix_ms();
        let run_id_c = run_id.clone();
        let dispatch_id_c = dispatch_id.clone();
        let cmd_c = cmd.clone();
        let exit_c = outcome.exit_code;
        let timed_c = outcome.timed_out;
        let tail_c = tail.clone();
        let head_c = head_sha.clone();
        commit(move |_| {
            Ok(Cmd::DispatchAppendVerification {
                run_id: run_id_c, dispatch_id: dispatch_id_c,
                command: cmd_c, exit_code: exit_c, timed_out: timed_c,
                tail: tail_c, head_sha: head_c, now,
            })
        }, run_id.clone()).await;

        let passed = !outcome.timed_out && outcome.exit_code == Some(0);
        if !passed {
            let tail_str = tail.join("\n");
            let seq = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(1)
            };
            let run_id_c = run_id.clone();
            let task_id_c = task_id.clone();
            let dispatch_id_c = dispatch_id.clone();
            let cmd_c = cmd.clone();
            let now2 = unix_ms();
            commit(move |_| {
                Ok(Cmd::TaskVerificationFailed {
                    run_id: run_id_c, task_id: task_id_c, dispatch_id: dispatch_id_c,
                    command: cmd_c, exit_code: exit_c, timed_out: timed_c,
                    tail: tail_str, msg_seq: seq, now: now2,
                })
            }, run_id.clone()).await;
            return;
        }
    }

    let seq = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(1)
    };
    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let dispatch_id_c = dispatch_id.clone();
    let now = unix_ms();
    commit(move |_| {
        Ok(Cmd::TaskVerified {
            run_id: run_id_c, task_id: task_id_c, dispatch_id: dispatch_id_c,
            msg_seq: seq, now,
        })
    }, run_id).await;
}

const SESSION_GRACE_SECS: u64 = 60;

pub(crate) struct TickFacts {
    pub is_session_live: bool,
    pub ever_live: bool,
    pub consecutive_shell_ticks: u8,
    pub screen_quiet_secs: u64,
    pub agent_status: Option<String>,
    pub running_secs: u64,
    pub is_waiting: bool,
    pub limits_dispatch_timeout_min: u32,
    pub limits_stall_quiet_min: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Finding {
    SessionGone,
    AgentExited,
    Timeout,
    Quiet,
}

impl Finding {
    fn as_str(&self) -> &str {
        match self {
            Finding::SessionGone => "session_gone",
            Finding::AgentExited => "agent_exited",
            Finding::Timeout => "timeout",
            Finding::Quiet => "quiet",
        }
    }
}

pub(crate) fn assess(facts: &TickFacts, sent: &std::collections::HashSet<String>) -> Vec<Finding> {
    let mut result = Vec::new();
    if !facts.is_session_live {
        let in_grace = !facts.ever_live && facts.running_secs < SESSION_GRACE_SECS;
        if !in_grace && !sent.contains("session_gone") {
            result.push(Finding::SessionGone);
        }
        return result;
    }
    if facts.consecutive_shell_ticks >= 2 && !sent.contains("agent_exited") {
        result.push(Finding::AgentExited);
    }
    let timeout_secs = facts.limits_dispatch_timeout_min as u64 * 60;
    if facts.running_secs > timeout_secs && !sent.contains("timeout") {
        result.push(Finding::Timeout);
    }
    let quiet_secs = facts.limits_stall_quiet_min as u64 * 60;
    if !facts.is_waiting
        && facts.screen_quiet_secs >= quiet_secs
        && facts.agent_status.as_deref() != Some("busy")
        && !sent.contains("quiet")
    {
        result.push(Finding::Quiet);
    }
    result
}

struct DispatchWatch {
    consecutive_shell_ticks: u8,
    ever_live: bool,
    last_screen: String,
    screen_last_changed_secs: u64,
    sent: std::collections::HashSet<String>,
}

fn supervision_watch() -> &'static Mutex<std::collections::HashMap<String, DispatchWatch>> {
    static MAP: std::sync::OnceLock<Mutex<std::collections::HashMap<String, DispatchWatch>>> =
        std::sync::OnceLock::new();
    MAP.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

async fn supervision_loop() {
    let mut interval = tokio::time::interval(Duration::from_secs(20));
    loop {
        interval.tick().await;
        supervision_tick().await;
    }
}

async fn supervision_tick() {
    let now_secs = unix_ms() / 1000;

    let starting_set = starting().lock().unwrap_or_else(|e| e.into_inner()).clone();
    let running_dispatches: Vec<(String, String, String, u64, Limits)> = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.values()
            .filter(|e| !e.run.status.is_terminal())
            .flat_map(|e| {
                let limits = e.run.limits.clone();
                e.dispatches.values()
                    .filter(|d| d.status == DispatchStatus::Running && !starting_set.contains(&d.id))
                    .filter_map(|d| d.tile_id.clone().map(|tid| (
                        e.run.id.clone(),
                        d.id.clone(),
                        tid,
                        d.started_at / 1000,
                        limits.clone(),
                    )))
                    .collect::<Vec<_>>()
            })
            .collect()
    };

    for (run_id, dispatch_id, tile_id, started_secs, limits) in running_dispatches {
        let session = crate::sessions().lock().unwrap().get(&tile_id).cloned();
        let is_session_live = session.is_some();

        let agent_status = session.as_ref()
            .and_then(|s| crate::cached_agent_state(s))
            .and_then(|js| serde_json::from_str::<serde_json::Value>(&js).ok())
            .and_then(|v| v["lastStatus"].as_str().map(str::to_string));

        let is_waiting = agent_status.as_deref() == Some("waiting");

        let (consecutive_shell_ticks, ever_live, screen_quiet_secs, screen_text) = {
            let mut watch = supervision_watch().lock().unwrap_or_else(|e| e.into_inner());
            let entry = watch.entry(dispatch_id.clone()).or_insert_with(|| DispatchWatch {
                consecutive_shell_ticks: 0,
                ever_live: false,
                last_screen: String::new(),
                screen_last_changed_secs: now_secs,
                sent: std::collections::HashSet::new(),
            });
            if is_session_live {
                entry.ever_live = true;
            }
            let screen = worker::capture_worker_screen(&tile_id, 40);
            if screen != entry.last_screen {
                entry.last_screen = screen.clone();
                entry.screen_last_changed_secs = now_secs;
                entry.consecutive_shell_ticks = 0;
            }
            let quiet = now_secs.saturating_sub(entry.screen_last_changed_secs);
            if is_session_live {
                let is_shell = session.as_ref()
                    .filter(|s| crate::session_pid(s) > 0)
                    .map(|s| {
                        let shell = crate::shell_name_for(s);
                        crate::foreground_name_for(s).map_or(true, |fg| fg == shell)
                    })
                    .unwrap_or(false);
                if is_shell {
                    entry.consecutive_shell_ticks = entry.consecutive_shell_ticks.saturating_add(1);
                } else {
                    entry.consecutive_shell_ticks = 0;
                }
            }
            (entry.consecutive_shell_ticks, entry.ever_live, quiet, screen)
        };

        let running_secs = now_secs.saturating_sub(started_secs);

        let sent = {
            let watch = supervision_watch().lock().unwrap_or_else(|e| e.into_inner());
            watch.get(&dispatch_id).map(|e| e.sent.clone()).unwrap_or_default()
        };

        let facts = TickFacts {
            is_session_live,
            ever_live,
            consecutive_shell_ticks,
            screen_quiet_secs,
            agent_status: agent_status.clone(),
            running_secs,
            is_waiting,
            limits_dispatch_timeout_min: limits.dispatch_timeout_min,
            limits_stall_quiet_min: limits.stall_quiet_min,
        };
        let findings = assess(&facts, &sent);

        for finding in findings {
            let reason = finding.as_str().to_string();
            {
                let mut watch = supervision_watch().lock().unwrap_or_else(|e| e.into_inner());
                if let Some(e) = watch.get_mut(&dispatch_id) {
                    e.sent.insert(reason.clone());
                }
            }

            match finding {
                Finding::SessionGone | Finding::AgentExited => {
                    let (summary, msg_body) = if finding == Finding::AgentExited {
                        let tail = screen_text.lines().rev().take(10).collect::<Vec<_>>()
                            .into_iter().rev().collect::<Vec<_>>().join("\n");
                        let s = format!("agent exited\n{tail}");
                        let b = serde_json::json!({"reason":"agent_exited","detail":"agent exited","tail":tail}).to_string();
                        (s, b)
                    } else {
                        let s = "worker session ended".to_string();
                        let b = serde_json::json!({"reason":"session_gone","detail":"worker session ended"}).to_string();
                        (s, b)
                    };
                    let now = unix_ms();
                    let run_id_c = run_id.clone();
                    let dispatch_id_c = dispatch_id.clone();
                    let summary_c = summary.clone();
                    commit(move |_| {
                        Ok(Cmd::DispatchUpdate {
                            run_id: run_id_c, dispatch_id: dispatch_id_c,
                            status: DispatchStatus::Failed,
                            summary: Some(summary_c),
                            now,
                        })
                    }, run_id.clone()).await;

                    let seq = {
                        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                        guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(1)
                    };
                    let run_id_c = run_id.clone();
                    let dispatch_id_c = dispatch_id.clone();
                    let now2 = unix_ms();
                    commit(move |g| {
                        let entry = g.runs.get(&run_id_c)
                            .ok_or_else(|| "not_found:run not found".to_string())?;
                        let task_id = entry.dispatches.get(&dispatch_id_c)
                            .map(|d| d.task_id.clone())
                            .ok_or_else(|| "not_found:dispatch not found".to_string())?;
                        Ok(Cmd::Send {
                            run_id: run_id_c.clone(),
                            seq,
                            from: "brain".into(),
                            to: vec!["coordinator".into()],
                            msg_type: "worker_failed".into(),
                            task_id: Some(task_id),
                            dispatch_id: Some(dispatch_id_c),
                            reply_to: None,
                            body: Some(msg_body),
                            outcome: None,
                            files_modified: vec![],
                            summary: None,
                            now: now2,
                        })
                    }, run_id.clone()).await;
                }
                Finding::Timeout | Finding::Quiet => {
                    let tail = screen_text.lines().rev().take(20).collect::<Vec<_>>()
                        .into_iter().rev().collect::<Vec<_>>().join("\n");
                    let body_str = serde_json::json!({"reason": reason, "tail": tail}).to_string();
                    let seq = {
                        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                        guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(1)
                    };
                    let run_id_c = run_id.clone();
                    let dispatch_id_c = dispatch_id.clone();
                    let now = unix_ms();
                    commit(move |g| {
                        let entry = g.runs.get(&run_id_c)
                            .ok_or_else(|| "not_found:run not found".to_string())?;
                        let task_id = entry.dispatches.get(&dispatch_id_c)
                            .map(|d| d.task_id.clone())
                            .ok_or_else(|| "not_found:dispatch not found".to_string())?;
                        Ok(Cmd::Send {
                            run_id: run_id_c.clone(),
                            seq,
                            from: "brain".into(),
                            to: vec!["coordinator".into()],
                            msg_type: "worker_stalled".into(),
                            task_id: Some(task_id),
                            dispatch_id: Some(dispatch_id_c),
                            reply_to: None,
                            body: Some(body_str),
                            outcome: None,
                            files_modified: vec![],
                            summary: None,
                            now,
                        })
                    }, run_id.clone()).await;
                }
            }
        }
    }
}

async fn handle_verify(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }

    let (dispatch_id, head_sha, needs_re_verify) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return err_resp("not_found", "task not found"),
        };
        if !matches!(task.status, TaskStatus::Verifying | TaskStatus::Failed) {
            return err_resp("conflict", "task must be verifying or failed");
        }
        if task.verify.is_empty() {
            return err_resp("bad_request", "task has no verify commands");
        }
        let latest = entry.dispatches.values()
            .filter(|d| d.task_id == task_id)
            .max_by_key(|d| d.attempt);
        let d = match latest {
            Some(d) => d,
            None => return err_resp("conflict", "no dispatch found"),
        };
        if d.status != DispatchStatus::Reported {
            return err_resp("conflict", "latest dispatch must be in reported state");
        }
        let needs_re = task.status == TaskStatus::Failed;
        (d.id.clone(), d.head_sha.clone(), needs_re)
    };

    if needs_re_verify {
        let now = unix_ms();
        let run_id_c = run_id.clone();
        let task_id_c = task_id.clone();
        let result = commit(move |_| {
            Ok(Cmd::TaskReVerify { run_id: run_id_c, task_id: task_id_c, now })
        }, run_id.clone()).await;
        if let CommitResult::Err(e) = result {
            return err_from_str(&e);
        }
    }

    let run_id2 = run_id.clone();
    let task_id2 = task_id.clone();
    let dispatch_id2 = dispatch_id.clone();
    tokio::spawn(async move {
        run_verification(run_id2, task_id2, dispatch_id2, head_sha).await;
    });
    ok_resp(serde_json::json!({"started": true}))
}

async fn handle_retry(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }

    let by_retry = v["by"].as_str().unwrap_or("coordinator");

    let (err, attempt, retry_of, escalation, task_profile, latest_profile, latest_model) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return err_resp("not_found", "task not found"),
        };
        if by_retry != "user" && task.owner.as_deref() == Some("user") {
            return err_resp("owned_by_user", "this task is owned by the user; only the user can retry it");
        }
        if task.status != TaskStatus::Failed {
            return err_resp("conflict", "task must be failed to retry");
        }
        let attempt = task.attempts + 1;
        let latest = entry.latest_dispatch_for_task(&task_id);
        let latest_profile = latest.and_then(|d| d.profile.clone());
        let latest_model = latest.and_then(|d| d.model.clone());
        let retry_of = latest.map(|d| d.id.clone());
        let escalation = task.escalation.clone();
        let task_profile = task.profile.clone();
        (None::<String>, attempt, retry_of, escalation, task_profile, latest_profile, latest_model)
    };
    if let Some(e) = err { return e; }

    let (profile_id, model) = pick_escalation(EscalationInput {
        escalation: &escalation,
        attempt,
        explicit_profile: v["profile"].as_str(),
        explicit_model: v["model"].as_str(),
        task_profile: task_profile.as_deref(),
        latest_profile: latest_profile.as_deref(),
        latest_model: latest_model.as_deref(),
    });

    if profile_id.is_empty() {
        return err_resp("bad_request", "no profile available for retry");
    }

    let mut inner_body = serde_json::json!({
        "runId": run_id,
        "taskId": task_id,
        "profile": profile_id,
        "by": by_retry,
    });
    if let Some(m) = model {
        inner_body["model"] = m.into();
    }
    if let Some(ro) = retry_of {
        inner_body["retryOf"] = ro.into();
    }
    handle_worker_start(inner_body.to_string().as_bytes()).await
}

fn build_report(entry: &crate::orch::reducer::RunEntry, integration_tip: Option<String>, format: &str) -> Value {
    use crate::orch::git::integration_branch;

    let mut tasks_by_status: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut dispatch_count = 0usize;
    let mut retry_count = 0usize;
    let mut worker_time_ms = 0u64;
    let mut total_exact_cost = 0f64;
    let mut unknown_cost_count = 0usize;
    let mut completed_exact_cost = 0f64;
    let mut completed_count = 0usize;

    let int_branch = integration_branch(&entry.run.id);

    let mut ordered_tasks: Vec<_> = entry.tasks.values().collect();
    ordered_tasks.sort_by_key(|t| t.id.trim_start_matches('t').parse::<u32>().unwrap_or(u32::MAX));
    let tasks_json: Vec<Value> = ordered_tasks.into_iter().map(|task| {
        let status_str = format!("{:?}", task.status).to_lowercase();
        *tasks_by_status.entry(status_str.clone()).or_insert(0) += 1;
        if task.status == TaskStatus::Completed { completed_count += 1; }

        let mut task_dispatches: Vec<_> = entry.dispatches.values().filter(|d| d.task_id == task.id).collect();
        task_dispatches.sort_by_key(|d| d.attempt);
        let dispatches_json: Vec<Value> = task_dispatches.into_iter()
            .map(|d| {
                dispatch_count += 1;
                if d.retry_of.is_some() { retry_count += 1; }
                let duration_ms = match (d.started_at, d.ended_at) {
                    (s, Some(e)) => e.saturating_sub(s),
                    _ => 0,
                };
                worker_time_ms += duration_ms;
                match (d.cost_source.as_deref(), d.cost_usd) {
                    (Some("exact"), Some(c)) => {
                        total_exact_cost += c;
                        if task.status == TaskStatus::Completed { completed_exact_cost += c; }
                    }
                    _ => { unknown_cost_count += 1; }
                }
                let verif_short: Vec<Value> = d.verifications.iter().map(|v| serde_json::json!({
                    "command": v.command,
                    "exitCode": v.exit_code,
                    "timedOut": v.timed_out,
                    "passed": !v.timed_out && v.exit_code == Some(0),
                })).collect();
                serde_json::json!({
                    "id": d.id,
                    "attempt": d.attempt,
                    "profile": d.profile,
                    "model": d.model,
                    "status": format!("{:?}", d.status).to_lowercase(),
                    "durationMs": duration_ms,
                    "verifications": verif_short,
                    "headSha": d.head_sha,
                    "costUsd": d.cost_usd,
                    "costSource": d.cost_source,
                })
            })
            .collect();

        let conflict_files: Vec<&str> = if task.status == TaskStatus::Blocked {
            task.result.as_deref()
                .map(|r| r.split(", ").collect())
                .unwrap_or_default()
        } else {
            vec![]
        };

        serde_json::json!({
            "id": task.id,
            "title": task.title,
            "status": status_str,
            "attempts": task.attempts,
            "dispatches": dispatches_json,
            "integratedSha": task.integrated_sha,
            "conflictFiles": conflict_files,
        })
    }).collect();

    let cost_per_completed = if completed_count > 0 {
        Some(completed_exact_cost / completed_count as f64)
    } else {
        None
    };

    let report_data = serde_json::json!({
        "run": {
            "id": entry.run.id,
            "title": entry.run.title,
            "status": format!("{:?}", entry.run.status).to_lowercase(),
            "baseSha": entry.run.base_sha,
            "integrationBranch": int_branch,
            "integrationTip": integration_tip,
        },
        "tasks": tasks_json,
        "totals": {
            "tasksByStatus": tasks_by_status,
            "dispatches": dispatch_count,
            "retries": retry_count,
            "workerTimeMs": worker_time_ms,
            "totalExactCostUsd": total_exact_cost,
            "unknownCostCount": unknown_cost_count,
            "costPerCompletedTask": cost_per_completed,
        }
    });

    if format == "markdown" {
        let md = render_report_markdown(&report_data);
        serde_json::json!({"text": md})
    } else {
        report_data
    }
}

fn verification_verdict(v: &Value) -> &'static str {
    if v["passed"].as_bool() == Some(true) {
        return "pass";
    }
    if v["timedOut"].as_bool() == Some(true) {
        return "timeout";
    }
    "fail"
}

fn render_report_markdown(data: &Value) -> String {
    let mut out = String::new();
    let run = &data["run"];
    out.push_str(&format!("# Run: {}\n\n", run["title"].as_str().unwrap_or("")));
    out.push_str(&format!("**Status:** {}  **Base:** {}\n\n",
        run["status"].as_str().unwrap_or(""),
        run["baseSha"].as_str().unwrap_or("none")));
    if let Some(tip) = run["integrationTip"].as_str() {
        out.push_str(&format!(
            "**Integration branch:** {} at {tip}\n\n",
            run["integrationBranch"].as_str().unwrap_or("")
        ));
    }
    out.push_str("## Tasks\n\n");
    if let Some(tasks) = data["tasks"].as_array() {
        for t in tasks {
            out.push_str(&format!("### {} ({})\n", t["title"].as_str().unwrap_or(""), t["status"].as_str().unwrap_or("")));
            out.push_str(&format!("Attempts: {}\n\n", t["attempts"]));
            if let Some(files) = t["conflictFiles"].as_array() {
                if !files.is_empty() {
                    let list: Vec<&str> = files.iter().filter_map(|f| f.as_str()).collect();
                    out.push_str(&format!("Conflict files: {}\n\n", list.join(", ")));
                }
            }
            if let Some(dispatches) = t["dispatches"].as_array() {
                for d in dispatches {
                    let profile = d["profile"].as_str().unwrap_or("?");
                    let agent = match d["model"].as_str() {
                        Some(model) => format!("{profile}:{model}"),
                        None => profile.to_string(),
                    };
                    let status = d["status"].as_str().unwrap_or("?");
                    let dur = d["durationMs"].as_u64().unwrap_or(0) / 1000;
                    out.push_str(&format!("- Dispatch {} ({agent}, {status}, {dur}s)", d["id"].as_str().unwrap_or("")));
                    match d["costUsd"].as_f64() {
                        Some(c) => out.push_str(&format!(" cost=${c:.4}")),
                        None => out.push_str(" cost=unknown"),
                    }
                    out.push('\n');
                    for v in d["verifications"].as_array().into_iter().flatten() {
                        out.push_str(&format!(
                            "  - verify `{}`: {} (exit {})\n",
                            v["command"].as_str().unwrap_or(""),
                            verification_verdict(v),
                            v["exitCode"]
                        ));
                    }
                }
            }
            out.push('\n');
        }
    }
    let totals = &data["totals"];
    out.push_str("## Totals\n\n");
    out.push_str(&format!("Dispatches: {}  Retries: {}  Worker time: {}ms\n",
        totals["dispatches"], totals["retries"], totals["workerTimeMs"]));
    out.push_str(&format!("Cost reported by agents: ${:.4}  Dispatches without a cost figure: {}\n",
        totals["totalExactCostUsd"].as_f64().unwrap_or(0.0),
        totals["unknownCostCount"]));
    if let Some(c) = totals["costPerCompletedTask"].as_f64() {
        out.push_str(&format!("Cost per completed task (reported figures only): ${c:.4}\n"));
    }
    out.push_str("\nCost figures are what each agent CLI reports (API-equivalent pricing). Subscription plans do not bill these amounts; dispatches without a figure are not estimated.\n");
    out
}

async fn handle_report(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let format = v["format"].as_str().unwrap_or("json").to_string();

    let (repo, run_id_c) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        match guard.runs.get(&run_id) {
            Some(e) => (e.run.repo.clone(), run_id.clone()),
            None => return err_resp("not_found", "run not found"),
        }
    };

    let int_branch = git::integration_branch(&run_id_c);
    let int_tip = tokio::task::spawn_blocking(move || {
        git::rev_parse(&repo, &int_branch).ok()
    }).await.unwrap_or(None);

    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let entry = match guard.runs.get(&run_id) {
        Some(e) => e,
        None => return err_resp("not_found", "run not found"),
    };
    let report = build_report(entry, int_tip, &format);
    drop(guard);

    ok_resp(report)
}

async fn undo_integration_merge(int_path: String) {
    let result = tokio::task::spawn_blocking(move || git::reset_hard(&int_path, "ORIG_HEAD")).await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => eprintln!("[orch] integrate: undo merge failed: {e}"),
        Err(e) => eprintln!("[orch] integrate: undo merge panicked: {e}"),
    }
}

async fn handle_integrate(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let explicit_dispatch_id = v["dispatchId"].as_str().map(str::to_string);

    let (repo, branch, dispatch_id_chosen, base_sha, task_status) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return err_resp("not_found", "task not found"),
        };
        let dispatch = if let Some(ref did) = explicit_dispatch_id {
            let d = match entry.dispatches.get(did.as_str()) {
                Some(d) => d,
                None => return err_resp("not_found", "dispatch not found"),
            };
            if d.task_id != task_id {
                return err_resp("not_found", "dispatch not found for this task");
            }
            if d.branch.is_none() {
                return err_resp("conflict", "dispatch has no branch");
            }
            d
        } else {
            let candidates: Vec<&Dispatch> = entry.dispatches.values()
                .filter(|d| d.task_id == task_id
                    && d.status == DispatchStatus::Reported
                    && d.outcome.as_deref() == Some("succeeded")
                    && d.branch.is_some())
                .collect();
            if candidates.is_empty() {
                return err_resp("conflict", "no reported dispatch with a branch; pass dispatchId explicitly");
            }
            if !task.verify.is_empty() {
                candidates.iter()
                    .filter(|d| !d.verifications.is_empty()
                        && d.verifications.iter().all(|vr| !vr.timed_out && vr.exit_code == Some(0)))
                    .max_by_key(|d| d.attempt)
                    .copied()
                    .or_else(|| candidates.iter().max_by_key(|d| d.attempt).copied())
                    .unwrap()
            } else {
                candidates.iter().max_by_key(|d| d.attempt).copied().unwrap()
            }
        };
        (entry.run.repo.clone(), dispatch.branch.clone().unwrap(), dispatch.id.clone(), entry.run.base_sha.clone(), task.status.clone())
    };

    if task_status != TaskStatus::Completed {
        return err_resp("conflict", "task must be completed to integrate");
    }

    let int_path = git::integration_worktree_path(&repo, &run_id);
    let int_branch = git::integration_branch(&run_id);
    let repo_c = repo.clone();
    let int_path_c = int_path.clone();
    let int_branch_c = int_branch.clone();
    let base_sha_c = base_sha.clone();

    let outcome = tokio::task::spawn_blocking(move || {
        if !std::path::Path::new(&int_path_c).exists() {
            let base = base_sha_c.as_deref().unwrap_or("HEAD");
            if let Err(e) = git::worktree_add(&repo_c, &int_path_c, &int_branch_c, base) {
                return Err(format!("integration worktree create failed: {e}"));
            }
        }
        Ok(git::merge_no_ff(&int_path_c, &branch))
    }).await;

    let merge_result = match outcome {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => return err_resp("conflict", &e),
        Err(e) => {
            eprintln!("[orch] integrate spawn_blocking panic: {e}");
            return err_resp("io", "integration spawn failed");
        }
    };

    match merge_result {
        Ok(git::MergeOutcome::Success(sha)) => {
            let run_id_c = run_id.clone();
            let task_id_c = task_id.clone();
            let dispatch_id_c = dispatch_id_chosen.clone();
            let now = unix_ms();
            let seq = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(0)
            };
            let result = commit(move |_| {
                Ok(Cmd::TaskIntegrate {
                    run_id: run_id_c,
                    task_id: task_id_c,
                    dispatch_id: dispatch_id_c,
                    integrated_sha: sha.clone(),
                    msg_seq: seq,
                    now,
                })
            }, run_id.clone()).await;
            match result {
                CommitResult::Ok(_) => ok_resp(serde_json::json!({"integrated": true})),
                CommitResult::Stale => {
                    undo_integration_merge(int_path.clone()).await;
                    err_resp("stale", "integrate stale")
                }
                CommitResult::Err(e) => {
                    undo_integration_merge(int_path.clone()).await;
                    err_from_str(&e)
                }
            }
        }
        Ok(git::MergeOutcome::Conflict(files)) => {
            let run_id_c = run_id.clone();
            let task_id_c = task_id.clone();
            let files_c = files.clone();
            let now = unix_ms();
            let seq = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(0)
            };
            let result = commit(move |_| {
                Ok(Cmd::TaskConflict {
                    run_id: run_id_c,
                    task_id: task_id_c,
                    files: files_c,
                    msg_seq: seq,
                    now,
                })
            }, run_id.clone()).await;
            let r = match result {
                CommitResult::Ok(_) | CommitResult::Stale => serde_json::json!({"conflict": true, "files": files}),
                CommitResult::Err(_) => serde_json::json!({"conflict": true, "files": files}),
            };
            ok_resp(r)
        }
        Err(e) => err_resp("conflict", &e),
    }
}

async fn handle_worktree_clean(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }

    let (repo, base_sha, candidates) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let candidates: Vec<CleanCandidate> = entry.dispatches.values()
            .filter_map(|d| Some(CleanCandidate {
                id: d.id.clone(),
                worktree: d.worktree.clone()?,
                branch: d.branch.clone()?,
                status: d.status.clone(),
                tile_id: d.tile_id.clone(),
            }))
            .collect();
        (entry.run.repo.clone(), entry.run.base_sha.clone(), candidates)
    };

    let int_branch = git::integration_branch(&run_id);
    let repo_v = repo.clone();
    let (removable, mut kept) = tokio::task::spawn_blocking(move || {
        let mut removable = vec![];
        let mut kept = vec![];
        for c in candidates {
            if !std::path::Path::new(&c.worktree).exists() { continue; }
            match clean_verdict(&c, &repo_v, &int_branch, base_sha.as_deref()) {
                Ok(()) => removable.push(c),
                Err(reason) => kept.push(serde_json::json!({"path": c.worktree, "reason": reason})),
            }
        }
        (removable, kept)
    }).await.unwrap_or_default();

    if release_worker_sessions(&removable).await {
        tokio::time::sleep(Duration::from_millis(SESSION_RELEASE_SETTLE_MS)).await;
    }

    let (removed, failed) = tokio::task::spawn_blocking(move || {
        let mut removed = vec![];
        let mut failed = vec![];
        for c in removable {
            match git::worktree_remove(&repo, &c.worktree) {
                Ok(()) => removed.push(c.worktree),
                Err(e) => failed.push(serde_json::json!({"path": c.worktree, "reason": format!("remove failed: {e}")})),
            }
        }
        (removed, failed)
    }).await.unwrap_or_default();
    kept.extend(failed);

    ok_resp(serde_json::json!({"removed": removed, "kept": kept}))
}

struct CleanCandidate {
    id: String,
    worktree: String,
    branch: String,
    status: DispatchStatus,
    tile_id: Option<String>,
}

// A live worker shell keeps its cwd open, and Windows refuses to delete that folder.
async fn release_worker_sessions(candidates: &[CleanCandidate]) -> bool {
    let mut released = false;
    for tile in candidates.iter().filter_map(|c| c.tile_id.as_deref()) {
        if let Some(session) = crate::lookup_live(tile) {
            crate::kill_session(&session);
            released = true;
        }
    }
    released
}

fn clean_verdict(c: &CleanCandidate, repo: &str, int_branch: &str, base_sha: Option<&str>) -> Result<(), String> {
    if c.status == DispatchStatus::Running {
        return Err(format!("dispatch {} still running", c.id));
    }
    if !git::is_registered_worktree(repo, &c.worktree) {
        return Ok(());
    }
    let status = git::status_porcelain(&c.worktree)?;
    if !status.trim().is_empty() {
        return Err("uncommitted changes".into());
    }
    if git::is_merged_into(repo, &c.branch, int_branch) {
        return Ok(());
    }
    let abandoned = matches!(c.status, DispatchStatus::Failed | DispatchStatus::Stopped);
    let has_commits = match base_sha {
        Some(base) => git::has_commits_beyond(&c.worktree, base)?,
        None => true,
    };
    if abandoned && !has_commits {
        return Ok(());
    }
    Err("branch not merged into the integration branch".into())
}

async fn handle_check(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let recipient = v["recipient"].as_str().unwrap_or("coordinator").to_string();
    let types: Option<Vec<String>> = match v.get("types").filter(|t| !t.is_null()) {
        None => None,
        Some(t) => match serde_json::from_value(t.clone()) {
            Ok(ts) => Some(ts),
            Err(_) => return err_resp("bad_request", "types must be array of strings"),
        }
    };
    let peek = v["peek"].as_bool().unwrap_or(false);
    let wait = v["wait"].as_bool().unwrap_or(false);
    let timeout_ms = v["timeoutMs"].as_u64().unwrap_or(30_000).min(600_000);

    {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        if !guard.runs.contains_key(run_id.as_str()) {
            return err_resp("not_found", "run not found");
        }
    }

    let notify = run_notify(&run_id);
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };

    let deadline = tokio::time::Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        // Enable Notified BEFORE collect to avoid missing notifications
        let mut n = Box::pin(notify.notified());
        n.as_mut().enable();

        let run_id_c = run_id.clone();
        let recipient_c = recipient.clone();
        let types_c = types.clone();
        let base_c = base.clone();
        let result = tokio::task::block_in_place(move || {
            collect_ack_sync(&run_id_c, &recipient_c, &types_c, peek, &base_c)
        });

        match result {
            Err(e) => return err_from_str(&e),
            Ok(msgs) if !msgs.is_empty() || !wait => {
                return ok_resp(serde_json::json!({"messages": msgs}));
            }
            Ok(_) => {}
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() { break; }
        let _ = tokio::time::timeout(remaining, n).await;
    }

    ok_resp(serde_json::json!({"messages": []}))
}

async fn handle_gate_create(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let question = v["question"].as_str().unwrap_or("").to_string();
    let options = str_arr_field(&v, "options");
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let gate_id = entry.next_gate_id();
        let msg_seq = entry.next_msg_seq();
        Ok(Cmd::GateCreate {
            run_id: run_id_c, id: gate_id, task_id: task_id_c,
            question, options, msg_seq, now,
        })
    }, run_id.clone()).await;
    match result {
        CommitResult::Ok(data) => {
            let gate_id = data["id"].as_str().unwrap_or("").to_string();
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let gate = guard.runs.get(&run_id)
                .and_then(|e| e.gates.get(&gate_id))
                .map(|g| serde_json::to_value(g).unwrap_or_default());
            ok_resp(serde_json::json!({"gate": gate}))
        }
        CommitResult::Stale => err_resp("stale", "gate create stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_gate_resolve(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let gate_id = v["gateId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || gate_id.is_empty() {
        return err_resp("bad_request", "runId and gateId required");
    }
    let resolution = v["resolution"].as_str().unwrap_or("").to_string();
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let gate_id_c = gate_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::GateResolve { run_id: run_id_c, gate_id: gate_id_c, resolution, now })
    }, run_id).await;
    match result {
        CommitResult::Ok(_) => ok_resp(serde_json::json!({})),
        CommitResult::Stale => err_resp("stale", "gate resolve stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

async fn handle_worker_start(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let explicit_profile: Option<String> = v["profile"].as_str().map(str::to_string);
    let explicit_model: Option<String> = v["model"].as_str().map(str::to_string);
    let retry_of: Option<String> = v["retryOf"].as_str().map(str::to_string);
    let by = v["by"].as_str().unwrap_or("coordinator").to_string();

    let (profile_id, model) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return err_resp("not_found", "task not found"),
        };
        if by != "user" && task.owner.as_deref() == Some("user") {
            return err_resp("owned_by_user", "this task is owned by the user; only the user can dispatch it");
        }
        let attempt = task.attempts + 1;
        let latest_profile = entry.latest_dispatch_for_task(&task_id).and_then(|d| d.profile.clone());
        let latest_model = entry.latest_dispatch_for_task(&task_id).and_then(|d| d.model.clone());
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &task.escalation,
            attempt,
            explicit_profile: explicit_profile.as_deref(),
            explicit_model: explicit_model.as_deref(),
            task_profile: task.profile.as_deref(),
            latest_profile: latest_profile.as_deref(),
            latest_model: latest_model.as_deref(),
        });
        if p.is_empty() {
            return err_resp("bad_request", "profile required (no escalation list and no task profile)");
        }
        (p, m)
    };
    let worktree_mode = v["worktree"].as_str().unwrap_or("auto").to_string();
    let stop_live = v["stopLive"].as_bool().unwrap_or(false);

    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };

    let profiles = load_profiles(&base);
    let Some(profile) = find_profile(&profiles, &profile_id) else {
        return err_resp("bad_request", &format!("unknown profile: {profile_id}"));
    };
    let profile = profile.clone();

    let live_dispatch_ids: Vec<(String, String)> = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        entry.dispatches.values()
            .filter(|d| d.task_id == task_id && d.accepts_reports())
            .filter_map(|d| {
                let tile = d.tile_id.as_ref()?;
                if crate::lookup_live(tile).is_some() {
                    Some((d.id.clone(), tile.clone()))
                } else {
                    None
                }
            })
            .collect()
    };

    if !live_dispatch_ids.is_empty() {
        if !stop_live {
            let first_id = &live_dispatch_ids[0].0;
            return err_resp("live_attempt", &format!("attempt {first_id} is still running"));
        }
        for (did, tile) in &live_dispatch_ids {
            let run_id_s = run_id.clone();
            let did_s = did.clone();
            let tile_s = tile.clone();
            stop_running_dispatch(run_id_s, did_s, tile_s).await.ok();
        }
    }

    let (owner_for_dispatch, task_attempts) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let attempts = guard.runs.get(&run_id)
            .and_then(|e| e.tasks.get(&task_id))
            .map(|t| t.attempts)
            .unwrap_or(0);
        let owner = if by == "user" && attempts == 0 { Some("user".to_string()) } else { None };
        (owner, attempts)
    };

    let tile_id = crate::ui::new_tile_id();
    let cli_path = crate::panorama_cli_path();
    let now = unix_ms();

    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let tile_id_c = tile_id.clone();
    let profile_id_c = profile_id.clone();
    let model_c = model.clone();
    let retry_of_c = retry_of.clone();
    let owner_c = owner_for_dispatch.clone();

    let result = commit(move |g| {
        let entry = g.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let task = entry.tasks.get(&task_id_c)
            .ok_or_else(|| "not_found:task not found".to_string())?;
        let attempt = task.attempts + 1;
        let dispatch_id = format!("{run_id_c}.{task_id_c}.{attempt}");
        Ok(Cmd::Dispatch {
            run_id: run_id_c.clone(),
            id: dispatch_id,
            task_id: task_id_c.clone(),
            attempt,
            retry_of: retry_of_c,
            profile: Some(profile_id_c.clone()),
            model: model_c,
            tile_id: Some(tile_id_c.clone()),
            owner: owner_c,
            now,
        })
    }, run_id.clone()).await;

    let dispatch_id = match result {
        CommitResult::Ok(data) => match data["id"].as_str() {
            Some(id) => id.to_string(),
            None => return err_resp("io", "dispatch id missing"),
        },
        CommitResult::Stale => return err_resp("stale", "dispatch stale"),
        CommitResult::Err(e) => return err_from_str(&e),
    };

    if by == "user" {
        let has_coordinator = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs.get(&run_id).is_some_and(|e| e.run.coordinator_tile.is_some())
        };
        if has_coordinator {
            let attempt_num = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                guard.runs.get(&run_id)
                    .and_then(|e| e.dispatches.get(&dispatch_id))
                    .map(|d| d.attempt)
                    .unwrap_or(task_attempts + 1)
            };
            let body_str = if attempt_num <= 1 {
                format!("user started {task_id} manually with {profile_id}; it is owned by the user, do not dispatch it")
            } else {
                format!("user restarted {task_id} manually with {profile_id} (attempt {attempt_num}); it is still yours to manage")
            };
            if let Err(e) = notify_coordinator_from_user(&run_id, &task_id, body_str).await {
                eprintln!("[orch] worker-start {run_id}/{task_id}: coordinator not notified: {e}");
            }
        }
    }

    let (task_clone, dispatch_clone, deps_results, base_cwd, run_base_sha, run_setup, task_has_deps, repo, run_title) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found after dispatch"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t.clone(),
            None => return err_resp("not_found", "task not found after dispatch"),
        };
        let dispatch = match entry.dispatches.get(&dispatch_id) {
            Some(d) => d.clone(),
            None => return err_resp("not_found", "dispatch not found after commit"),
        };
        let deps_results: Vec<(String, Option<String>)> = task.deps.iter().filter_map(|dep_id| {
            let dep = entry.tasks.get(dep_id)?;
            Some((dep_id.clone(), dep.result.clone()))
        }).collect();
        let cwd = v["cwd"].as_str().map(str::to_string)
            .unwrap_or_else(|| entry.run.repo.clone());
        let has_deps = !task.deps.is_empty();
        let title = entry.run.title.clone();
        (task, dispatch, deps_results, cwd, entry.run.base_sha.clone(), entry.run.setup.clone(), has_deps, entry.run.repo.clone(), title)
    };

    starting().lock().unwrap_or_else(|e| e.into_inner()).insert(dispatch_id.clone());

    let sd_input = StartDispatchInput {
        run_id: run_id.clone(),
        task_id: task_id.clone(),
        dispatch_id: dispatch_id.clone(),
        tile_id: tile_id.clone(),
        profile,
        model,
        base,
        cli_path,
        worktree_mode,
        base_cwd,
        run_base_sha,
        run_setup,
        task_has_deps,
        repo,
        run_title,
        task_clone,
        dispatch_clone,
        deps_results,
    };
    tokio::spawn(async move {
        start_dispatch(sd_input).await;
    });

    let dispatch_json = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.dispatches.get(&dispatch_id))
            .and_then(|d| serde_json::to_value(d).ok())
            .unwrap_or(serde_json::json!({"id": dispatch_id}))
    };
    ok_resp(serde_json::json!({"dispatch": dispatch_json, "starting": true}))
}

struct StartDispatchInput {
    run_id: String,
    task_id: String,
    dispatch_id: String,
    tile_id: String,
    profile: crate::orch::profiles::Profile,
    model: Option<String>,
    base: std::path::PathBuf,
    cli_path: String,
    worktree_mode: String,
    base_cwd: String,
    run_base_sha: Option<String>,
    run_setup: Option<String>,
    task_has_deps: bool,
    repo: String,
    run_title: String,
    task_clone: Task,
    dispatch_clone: Dispatch,
    deps_results: Vec<(String, Option<String>)>,
}

struct FailDispatchInput {
    run_id: String,
    task_id: String,
    dispatch_id: String,
    summary: String,
    msg_type: String,
    body: String,
}

async fn fail_dispatch_and_message_coordinator(input: FailDispatchInput) {
    let FailDispatchInput { run_id, task_id, dispatch_id, summary, msg_type, body } = input;
    let now = unix_ms();
    let run_id2 = run_id.clone();
    let dispatch_id2 = dispatch_id.clone();
    let summary2 = summary.clone();
    commit(move |_| {
        Ok(Cmd::DispatchUpdate {
            run_id: run_id2, dispatch_id: dispatch_id2,
            status: DispatchStatus::Failed,
            summary: Some(summary2),
            now,
        })
    }, run_id.clone()).await;

    let seq = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(1)
    };
    let run_id3 = run_id.clone();
    let dispatch_id3 = dispatch_id.clone();
    let task_id3 = task_id.clone();
    let now2 = unix_ms();
    commit(move |g| {
        let entry = g.runs.get(&run_id3)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let task_id_resolved = entry.dispatches.get(&dispatch_id3)
            .map(|d| d.task_id.clone())
            .unwrap_or(task_id3);
        let seq2 = entry.next_msg_seq().max(seq);
        Ok(Cmd::Send {
            run_id: run_id3,
            seq: seq2,
            from: "brain".into(),
            to: vec!["coordinator".into()],
            msg_type,
            task_id: Some(task_id_resolved),
            dispatch_id: Some(dispatch_id3),
            reply_to: None,
            body: Some(body),
            outcome: None,
            files_modified: vec![],
            summary: None,
            now: now2,
        })
    }, run_id).await;
}

async fn start_dispatch(input: StartDispatchInput) {
    let StartDispatchInput {
        run_id, task_id, dispatch_id, tile_id, profile, model, base, cli_path,
        worktree_mode, base_cwd, run_base_sha, run_setup, task_has_deps, repo,
        run_title, task_clone, dispatch_clone, deps_results,
    } = input;

    let remove_starting = || {
        starting().lock().unwrap_or_else(|e| e.into_inner()).remove(&dispatch_id);
    };

    macro_rules! bail_fail {
        ($summary:expr, $msg_type:expr, $body:expr) => {{
            fail_dispatch_and_message_coordinator(FailDispatchInput {
                run_id: run_id.clone(),
                task_id: task_id.clone(),
                dispatch_id: dispatch_id.clone(),
                summary: $summary,
                msg_type: $msg_type,
                body: $body,
            }).await;
            remove_starting();
            return;
        }};
    }

    let is_running = || {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.dispatches.get(&dispatch_id))
            .map(|d| d.status == DispatchStatus::Running)
            .unwrap_or(false)
    };

    if !is_running() {
        remove_starting();
        return;
    }

    let use_worktree = worktree_mode != "none" && run_base_sha.is_some();
    let cwd = if use_worktree {
        let base_sha_val = run_base_sha.clone().unwrap_or_default();
        let int_branch = git::integration_branch(&run_id);
        let repo_c = repo.clone();
        let int_br_c = int_branch.clone();
        let base_for_wt = tokio::task::spawn_blocking(move || {
            if task_has_deps {
                git::rev_parse(&repo_c, &int_br_c).unwrap_or(base_sha_val)
            } else {
                base_sha_val
            }
        }).await.unwrap_or_else(|_| run_base_sha.clone().unwrap_or_default());

        if !is_running() { remove_starting(); return; }

        let wt_path = git::worktree_path(&repo, &run_id, &task_id, dispatch_clone.attempt);
        let wt_branch = git::worktree_branch(&run_id, &task_id, dispatch_clone.attempt);

        let repo_c2 = repo.clone();
        let wt_path_c = wt_path.clone();
        let wt_branch_c = wt_branch.clone();
        let wt_result = tokio::task::spawn_blocking(move || {
            git::worktree_add(&repo_c2, &wt_path_c, &wt_branch_c, &base_for_wt)
        }).await;

        match wt_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                bail_fail!(
                    format!("worktree create failed: {e}"),
                    "worker_failed".into(),
                    serde_json::json!({"reason":"worktree_failed","detail":e}).to_string()
                );
            }
            Err(e) => {
                bail_fail!(
                    "worktree create: spawn failed".into(),
                    "worker_failed".into(),
                    serde_json::json!({"reason":"worktree_failed","detail":format!("spawn panic: {e}")}).to_string()
                );
            }
        }

        let dispatch_id_wt = dispatch_id.clone();
        let run_id_wt = run_id.clone();
        let wt_path_c2 = wt_path.clone();
        let wt_branch_c2 = wt_branch.clone();
        let now_wt = unix_ms();
        commit(move |_| {
            Ok(Cmd::DispatchSetWorktree {
                run_id: run_id_wt,
                dispatch_id: dispatch_id_wt,
                worktree: wt_path_c2,
                branch: wt_branch_c2,
                now: now_wt,
            })
        }, run_id.clone()).await;

        if let Some(setup_cmd) = run_setup {
            if !is_running() { remove_starting(); return; }
            use crate::orch::exec::{RunToExitInput, run_to_exit};
            let setup_input = RunToExitInput {
                cwd: wt_path.clone(),
                cmd: setup_cmd,
                timeout: std::time::Duration::from_secs(600),
                dispatch_id: dispatch_id.clone(),
                purpose: "setup".into(),
            };
            match run_to_exit(setup_input).await {
                Ok(outcome) if !outcome.timed_out && outcome.exit_code == Some(0) => {}
                Ok(outcome) => {
                    let tail_lines: Vec<&str> = outcome.tail.iter()
                        .rev().take(40).rev().map(|s| s.as_str()).collect();
                    let tail_str = tail_lines.join("\n");
                    let detail = if outcome.timed_out {
                        "setup timed out".to_string()
                    } else {
                        format!("setup exited {:?}", outcome.exit_code)
                    };
                    bail_fail!(
                        format!("{detail}\n{tail_str}"),
                        "setup_failed".into(),
                        serde_json::json!({"reason":"setup_failed","detail":detail,"tail":tail_str}).to_string()
                    );
                }
                Err(e) => {
                    bail_fail!(
                        format!("setup start failed: {e}"),
                        "setup_failed".into(),
                        serde_json::json!({"reason":"setup_failed","detail":e}).to_string()
                    );
                }
            }
        }

        wt_path
    } else {
        base_cwd
    };

    if !is_running() { remove_starting(); return; }

    let dispatch_for_brief = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.dispatches.get(&dispatch_id))
            .cloned()
            .unwrap_or(dispatch_clone)
    };

    let brief_path = match write_brief(BriefInput {
        base: &base,
        run_id: &run_id,
        dispatch_id: &dispatch_id,
        task: &task_clone,
        dispatch: &dispatch_for_brief,
        deps_results: &deps_results,
        cli_path: &cli_path,
        cwd: &cwd,
    }) {
        Ok(p) => p,
        Err(e) => {
            bail_fail!(
                format!("brief write failed: {e}"),
                "worker_failed".into(),
                serde_json::json!({"reason":"brief_failed","detail":e}).to_string()
            );
        }
    };

    let settings_path = if profile.exe == "claude" {
        match crate::orch::worker::write_worker_settings(&brief_path, &task_clone.verify) {
            Ok(p) => Some(p.to_string_lossy().into_owned()),
            Err(e) => {
                eprintln!("[orch] {dispatch_id}: worker permission settings skipped: {e}");
                None
            }
        }
    } else {
        None
    };

    let extra_env: Vec<(String, String)> = {
        let mut env = resolve_env(&profile.env, Some(&base));
        env.push(("PANORAMA_RUN_ID".into(), run_id.clone()));
        env.push(("PANORAMA_DISPATCH_ID".into(), dispatch_id.clone()));
        env
    };

    if !is_running() { remove_starting(); return; }

    let launch = WorkerLaunch {
        tile_id: &tile_id,
        cwd: &cwd,
        group: Some(TileGroup { id: &run_id, title: &run_title }),
        profile: &profile,
        model: model.as_deref(),
        extra_env,
        brief_path: &brief_path,
        settings_path,
    };
    if let Err(e) = spawn_worker(launch).await {
        bail_fail!(
            format!("spawn failed: {e}"),
            "worker_failed".into(),
            serde_json::json!({"reason":"spawn_failed","detail":e}).to_string()
        );
    }
    remove_starting();
}

fn handle_worker_show(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("");
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("");
    if run_id.is_empty() || dispatch_id.is_empty() {
        return err_resp("bad_request", "runId and dispatchId required");
    }
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = guard.runs.get(run_id) else {
        return err_resp("not_found", "run not found");
    };
    let Some(dispatch) = entry.dispatches.get(dispatch_id) else {
        return err_resp("not_found", "dispatch not found");
    };
    let tile_id = dispatch.tile_id.clone().unwrap_or_default();
    let live = !tile_id.is_empty() && crate::lookup_live(&tile_id).is_some();
    let agent_state = if !tile_id.is_empty() {
        crate::sessions().lock().unwrap().get(&tile_id).cloned()
            .and_then(|s| crate::cached_agent_state(&s))
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
    } else {
        None
    };
    let dispatch_val = serde_json::to_value(dispatch).unwrap_or(serde_json::json!({}));
    let mut resp = serde_json::json!({ "dispatch": dispatch_val, "live": live });
    if let Some(state) = agent_state {
        resp["agentState"] = state;
    }
    ok_resp(resp)
}

fn handle_worker_read(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("");
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("");
    if run_id.is_empty() || dispatch_id.is_empty() {
        return err_resp("bad_request", "runId and dispatchId required");
    }
    let lines = v["lines"].as_u64().unwrap_or(80) as usize;
    let tile_id = match dispatch_tile(run_id, dispatch_id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let text = capture_worker_screen(&tile_id, lines);
    ok_resp(serde_json::json!({"text": text}))
}

fn dispatch_tile(run_id: &str, dispatch_id: &str) -> Result<String, String> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let entry = guard.runs.get(run_id).ok_or_else(|| err_resp("not_found", "run not found"))?;
    let dispatch = entry.dispatches.get(dispatch_id)
        .ok_or_else(|| err_resp("not_found", "dispatch not found"))?;
    dispatch.tile_id.clone().filter(|t| !t.is_empty())
        .ok_or_else(|| err_resp("bad_request", "dispatch has no tile"))
}

fn handle_worker_send(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("");
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("");
    let text = v["text"].as_str().unwrap_or("");
    if run_id.is_empty() || dispatch_id.is_empty() || text.is_empty() {
        return err_resp("bad_request", "runId, dispatchId and text required");
    }
    let tile_id = match dispatch_tile(run_id, dispatch_id) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if !v["keys"].as_bool().unwrap_or(false) {
        return match crate::send_to_tile(&tile_id, text) {
            Ok(()) => ok_resp(serde_json::json!({})),
            Err(e) => err_resp("conflict", &e),
        };
    }
    let Some(session) = crate::lookup_live(&tile_id) else {
        return err_resp("conflict", "worker session not running");
    };
    crate::write_input(&session, text);
    ok_resp(serde_json::json!({}))
}

async fn stop_running_dispatch(run_id: String, dispatch_id: String, tile_id: String) -> Result<(), String> {
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let dispatch_id_c = dispatch_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::DispatchUpdate {
            run_id: run_id_c, dispatch_id: dispatch_id_c,
            status: DispatchStatus::Stopped,
            summary: None, now,
        })
    }, run_id).await;
    match result {
        CommitResult::Ok(_) | CommitResult::Stale => {}
        CommitResult::Err(e) => return Err(e),
    }
    if !tile_id.is_empty() {
        if let Some(s) = crate::sessions().lock().unwrap().get(&tile_id).cloned() {
            crate::kill_session(&s);
        }
    }
    Ok(())
}

async fn handle_worker_stop(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || dispatch_id.is_empty() {
        return err_resp("bad_request", "runId and dispatchId required");
    }
    let (tile_id, can_commit_stopped, has_live_tile) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let d = guard.runs.get(&run_id).and_then(|e| e.dispatches.get(&dispatch_id));
        let tile = d.and_then(|d| d.tile_id.clone()).unwrap_or_default();
        let live = !tile.is_empty() && crate::lookup_live(&tile).is_some();
        let stoppable = d.map(|d| matches!(d.status, DispatchStatus::Running | DispatchStatus::Reported)).unwrap_or(false);
        (tile, stoppable, live)
    };
    if !can_commit_stopped && !has_live_tile {
        return err_resp("conflict", "dispatch is not running and has no live tile");
    }
    if can_commit_stopped {
        let now = unix_ms();
        let run_id2 = run_id.clone();
        let dispatch_id2 = dispatch_id.clone();
        let result = commit(move |_| {
            Ok(Cmd::DispatchUpdate {
                run_id: run_id2, dispatch_id: dispatch_id2,
                status: DispatchStatus::Stopped,
                summary: None, now,
            })
        }, run_id.clone()).await;
        if let CommitResult::Err(e) = result {
            return err_from_str(&e);
        }
    }
    if !tile_id.is_empty() {
        if let Some(s) = crate::sessions().lock().unwrap().get(&tile_id).cloned() {
            crate::kill_session(&s);
        }
    }
    ok_resp(serde_json::json!({}))
}

async fn handle_task_cancel(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let (task_title, dispatch_info) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(&run_id) else {
            return err_resp("not_found", "run not found");
        };
        let Some(task) = entry.tasks.get(&task_id) else {
            return err_resp("not_found", "task not found");
        };
        if task.status.is_terminal() {
            return err_resp("conflict", "task is already terminal");
        }
        let title = task.title.clone();
        let dispatch_info = entry.latest_dispatch_for_task(&task_id)
            .filter(|d| matches!(d.status, DispatchStatus::Running | DispatchStatus::Reported))
            .map(|d| (d.id.clone(), d.tile_id.clone().unwrap_or_default()));
        (title, dispatch_info)
    };
    if let Some((dispatch_id, tile_id)) = dispatch_info {
        if let Err(e) = stop_running_dispatch(run_id.clone(), dispatch_id, tile_id).await {
            return err_from_str(&e);
        }
    }
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let cancel_result = commit(move |_| {
        Ok(Cmd::TaskUpdate {
            run_id: run_id_c, task_id: task_id_c,
            status: Some(TaskStatus::Cancelled),
            result: None, spec: None, acceptance: None,
            verify: None, profile: None, escalation: None, review: None, now,
        })
    }, run_id.clone()).await;
    if let CommitResult::Err(e) = cancel_result {
        return err_from_str(&e);
    }
    let body_str = format!(
        "User cancelled task {task_id} ({task_title}) from the run board. Do not retry it unless the user asks."
    );
    if let Err(e) = notify_coordinator_from_user(&run_id, &task_id, body_str).await {
        return err_resp("io", &format!("task cancelled, but the coordinator was not notified: {e}"));
    }
    ok_resp(serde_json::json!({}))
}

async fn notify_coordinator_from_user(run_id: &str, task_id: &str, body: String) -> Result<(), String> {
    let now = unix_ms();
    let run_id_c = run_id.to_string();
    let task_id_c = task_id.to_string();
    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let seq = entry.next_msg_seq();
        Ok(Cmd::Send {
            run_id: run_id_c, seq,
            from: "user".to_string(),
            to: vec!["coordinator".to_string()],
            msg_type: "note".to_string(),
            task_id: Some(task_id_c),
            dispatch_id: None, reply_to: None,
            body: Some(body),
            outcome: None, files_modified: vec![], summary: None, now,
        })
    }, run_id.to_string()).await;
    match result {
        CommitResult::Err(e) => Err(e),
        _ => Ok(()),
    }
}

async fn handle_worker_release(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || dispatch_id.is_empty() {
        return err_resp("bad_request", "runId and dispatchId required");
    }
    let (tile_id, is_terminal) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let d = guard.runs.get(&run_id).and_then(|e| e.dispatches.get(&dispatch_id));
        match d {
            Some(d) => (
                d.tile_id.clone().unwrap_or_default(),
                d.status != DispatchStatus::Running,
            ),
            None => return err_resp("not_found", "dispatch not found"),
        }
    };
    if !is_terminal {
        return err_resp("conflict", "dispatch must be in a terminal state to release");
    }
    if !tile_id.is_empty() {
        if let Some(s) = crate::sessions().lock().unwrap().get(&tile_id).cloned() {
            crate::kill_session(&s);
        }
    }
    ok_resp(serde_json::json!({}))
}

fn settings_with_keys_json(s: &settings::OrchSettings, base: Option<&std::path::Path>) -> Value {
    let providers: Vec<Value> = s.providers.iter().map(|p| {
        let has_key = secrets::get(&p.id, base).is_some();
        serde_json::json!({
            "id": p.id,
            "preset": p.preset,
            "label": p.label,
            "baseUrl": p.base_url,
            "mainModel": p.main_model,
            "fastModel": p.fast_model,
            "hasKey": has_key,
        })
    }).collect();
    serde_json::json!({
        "coordinator": s.coordinator,
        "roles": {
            "explore": s.roles.explore,
            "implement": s.roles.implement,
            "test": s.roles.test,
            "review": s.roles.review,
            "mechanical": s.roles.mechanical,
        },
        "orchestrateHint": s.orchestrate_hint,
        "hiddenAgents": s.hidden_agents,
        "providers": providers,
        "showWorkerTiles": s.show_worker_tiles,
        "customModels": s.custom_models,
        "aiActions": {
            "default": {
                "provider": s.ai_actions.default.provider,
                "model": s.ai_actions.default.model,
            },
            "overrides": {
                "commitMessage": s.ai_actions.overrides.commit_message.as_ref().map(|t| {
                    serde_json::json!({"provider": t.provider, "model": t.model})
                }),
                "tidy": s.ai_actions.overrides.tidy.as_ref().map(|t| {
                    serde_json::json!({"provider": t.provider, "model": t.model})
                }),
            }
        },
    })
}

fn presets_json() -> Value {
    settings::presets().iter().map(|p| {
        serde_json::json!({
            "id": p.id,
            "label": p.label,
            "baseUrl": p.base_url,
            "mainModel": p.main_model,
            "fastModel": p.fast_model,
        })
    }).collect::<Vec<_>>().into()
}

fn handle_settings_get() -> String {
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let s = settings::load(&base);
    let settings_json = settings_with_keys_json(&s, Some(&base));
    ok_resp(serde_json::json!({ "settings": settings_json, "presets": presets_json() }))
}

fn validate_chain(chain: &[String], known_ids: &[String]) -> Result<(), String> {
    for entry in chain {
        let profile = entry.split_once(':').map(|(p, _)| p).unwrap_or(entry);
        if !known_ids.iter().any(|id| id == profile) {
            return Err(format!("unknown agent profile: {profile}"));
        }
    }
    Ok(())
}

fn known_profile_ids(base: &std::path::Path) -> Vec<String> {
    let profiles = load_profiles(base);
    profiles.iter().map(|p| p.id.clone()).collect()
}

fn handle_settings_set(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let mut s = settings::load(&base);
    if let Some(coord) = v["coordinator"].as_str() {
        s.coordinator = coord.to_string();
    }
    if let Some(arr) = v["hiddenAgents"].as_array() {
        s.hidden_agents = arr.iter().filter_map(|x| x.as_str()).map(str::to_string).collect();
    }
    if let Some(show) = v["showWorkerTiles"].as_bool() {
        s.show_worker_tiles = show;
    }
    if let Some(hint_str) = v["orchestrateHint"].as_str() {
        match hint_str {
            "off" => s.orchestrate_hint = settings::OrchestrateHint::Off,
            "suggest" => s.orchestrate_hint = settings::OrchestrateHint::Suggest,
            "auto" => s.orchestrate_hint = settings::OrchestrateHint::Auto,
            _ => return err_resp("bad_hint", &format!("orchestrateHint must be off, suggest, or auto; got: {hint_str}")),
        }
    }
    if let Some(roles_obj) = v.get("roles").filter(|r| r.is_object()) {
        let ids = known_profile_ids(&base);
        let role_names = ["explore", "implement", "test", "review", "mechanical"];
        for role_name in &role_names {
            if let Some(arr) = roles_obj[role_name].as_array() {
                let chain: Vec<String> = arr.iter().filter_map(|x| x.as_str()).map(str::to_string).collect();
                if let Err(e) = validate_chain(&chain, &ids) {
                    return err_resp("bad_agent", &e);
                }
                match *role_name {
                    "explore" => s.roles.explore = chain,
                    "implement" => s.roles.implement = chain,
                    "test" => s.roles.test = chain,
                    "review" => s.roles.review = chain,
                    "mechanical" => s.roles.mechanical = chain,
                    _ => {}
                }
            }
        }
    }
    if let Some(map) = v.get("customModels").filter(|v| v.is_object()) {
        let mut new_map = std::collections::BTreeMap::new();
        for (agent_id, arr) in map.as_object().unwrap() {
            let models: Vec<String> = arr.as_array()
                .map(|a| a.iter()
                    .filter_map(|x| x.as_str())
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty())
                    .collect())
                .unwrap_or_default();
            if !models.is_empty() {
                let mut deduped = models;
                deduped.dedup();
                new_map.insert(agent_id.clone(), deduped);
            }
        }
        s.custom_models = new_map;
    }
    if let Some(ai_obj) = v.get("aiActions").filter(|v| v.is_object()) {
        if let Some(def_obj) = ai_obj.get("default").filter(|v| v.is_object()) {
            if let Some(p) = def_obj["provider"].as_str() {
                s.ai_actions.default.provider = p.to_string();
            }
            if let Some(m) = def_obj["model"].as_str() {
                s.ai_actions.default.model = m.to_string();
            }
        }
        if let Some(ov_obj) = ai_obj.get("overrides").filter(|v| v.is_object()) {
            if ov_obj.get("commitMessage").map(|v| v.is_null()).unwrap_or(false) {
                s.ai_actions.overrides.commit_message = None;
            } else if let Some(cm) = ov_obj.get("commitMessage").filter(|v| v.is_object()) {
                s.ai_actions.overrides.commit_message = Some(settings::AiTarget {
                    provider: cm["provider"].as_str().unwrap_or("claude").to_string(),
                    model: cm["model"].as_str().unwrap_or("haiku").to_string(),
                });
            }
            if ov_obj.get("tidy").map(|v| v.is_null()).unwrap_or(false) {
                s.ai_actions.overrides.tidy = None;
            } else if let Some(td) = ov_obj.get("tidy").filter(|v| v.is_object()) {
                s.ai_actions.overrides.tidy = Some(settings::AiTarget {
                    provider: td["provider"].as_str().unwrap_or("claude").to_string(),
                    model: td["model"].as_str().unwrap_or("haiku").to_string(),
                });
            }
        }
    }
    if let Err(e) = settings::save(&base, &s) {
        return err_resp("io", &format!("save settings: {e}"));
    }
    let settings_json = settings_with_keys_json(&s, Some(&base));
    ok_resp(serde_json::json!({ "settings": settings_json }))
}

fn handle_provider_save(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let preset = v["preset"].as_str().unwrap_or("").to_string();
    let label = v["label"].as_str().unwrap_or("").to_string();
    let base_url = v["baseUrl"].as_str().unwrap_or("").to_string();
    let main_model = v["mainModel"].as_str().unwrap_or("").to_string();
    let fast_model = v["fastModel"].as_str().unwrap_or("").to_string();
    let api_key: Option<String> = v["apiKey"].as_str().filter(|s| !s.is_empty()).map(str::to_string);

    if preset.is_empty() || label.is_empty() {
        return err_resp("bad_request", "preset and label required");
    }

    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };

    let mut s = settings::load(&base);

    let id = if let Some(explicit_id) = v["id"].as_str().filter(|s| !s.is_empty()) {
        if !s.providers.iter().any(|p| p.id == explicit_id) {
            return err_resp("not_found", &format!("provider not found: {explicit_id}"));
        }
        explicit_id.to_string()
    } else {
        let existing_ids: Vec<String> = s.providers.iter().map(|p| p.id.clone()).collect();
        settings::provider_id_for_label(&label, &existing_ids)
    };

    if let Some(key) = api_key {
        if let Err(e) = secrets::set(&id, &key, Some(&base)) {
            return err_resp("io", &format!("store key: {e}"));
        }
    }

    let provider = settings::OrchProvider { id: id.clone(), preset, label, base_url, main_model, fast_model };

    if let Some(existing) = s.providers.iter_mut().find(|p| p.id == id) {
        *existing = provider.clone();
    } else {
        s.providers.push(provider.clone());
    }

    if let Err(e) = settings::save(&base, &s) {
        return err_resp("io", &format!("save settings: {e}"));
    }

    let has_key = secrets::get(&id, Some(&base)).is_some();
    ok_resp(serde_json::json!({
        "provider": {
            "id": provider.id,
            "preset": provider.preset,
            "label": provider.label,
            "baseUrl": provider.base_url,
            "mainModel": provider.main_model,
            "fastModel": provider.fast_model,
            "hasKey": has_key,
        }
    }))
}

fn handle_provider_delete(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let id = v["id"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return err_resp("bad_request", "id required");
    }
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let mut s = settings::load(&base);
    s.providers.retain(|p| p.id != id);
    if let Err(e) = settings::save(&base, &s) {
        return err_resp("io", &format!("save settings: {e}"));
    }
    secrets::delete(&id, Some(&base));
    ok_resp(serde_json::json!({}))
}

async fn handle_provider_test(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let id = v["id"].as_str().unwrap_or("").to_string();
    if id.is_empty() {
        return err_resp("bad_request", "id required");
    }
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let s = settings::load(&base);
    let Some(provider) = s.providers.iter().find(|p| p.id == id) else {
        return err_resp("not_found", "provider not found");
    };
    let url = format!("{}/v1/messages", provider.base_url.trim_end_matches('/'));
    let Some(key) = secrets::get(&id, Some(&base)) else {
        return ok_resp(serde_json::json!({ "reachable": false, "message": "no API key stored" }));
    };
    let fast_model = provider.fast_model.clone();
    let result = tokio::task::spawn_blocking(move || {
        let req_body = serde_json::json!({
            "model": fast_model,
            "max_tokens": 1,
            "messages": [{"role": "user", "content": "ping"}]
        });
        ureq::post(&url)
            .timeout(std::time::Duration::from_secs(20))
            .set("Authorization", &format!("Bearer {key}"))
            .set("anthropic-version", "2023-06-01")
            .set("Content-Type", "application/json")
            .send_string(&req_body.to_string())
    }).await;

    match result {
        Ok(Ok(_)) => ok_resp(serde_json::json!({ "reachable": true, "message": "ok" })),
        Ok(Err(ureq::Error::Status(code, response))) => {
            let excerpt: String = response.into_string().unwrap_or_default().chars().take(200).collect();
            ok_resp(serde_json::json!({ "reachable": false, "status": code, "message": excerpt }))
        }
        Ok(Err(e)) => ok_resp(serde_json::json!({ "reachable": false, "message": e.to_string() })),
        Err(_) => err_resp("io", "spawn_blocking panic"),
    }
}

async fn handle_agent_list(body: &[u8]) -> String {
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let refresh = parse_body(body).ok()
        .and_then(|v| v["refresh"].as_bool())
        .unwrap_or(false);

    let s = settings::load(&base);
    let profiles = load_profiles(&base);

    let available_builtin_ids: Vec<String> = profiles.iter()
        .filter(|p| !p.provider && crate::command_exists(&p.exe))
        .map(|p| p.id.clone())
        .collect();

    let base_c = base.clone();
    let prepared = tokio::task::spawn_blocking(move || model_catalog::prepare(available_builtin_ids, base_c, refresh)).await;
    if prepared.is_err() {
        return err_resp("io", "model discovery panicked");
    }

    let agents: Vec<Value> = profiles.iter().map(|profile| {
        if profile.provider {
            let provider = s.providers.iter().find(|p| p.id == profile.id);
            let label = provider.map(|p| p.label.as_str()).unwrap_or(&profile.id).to_string();
            let has_key = secrets::get(&profile.id, Some(&base)).is_some();
            let available = has_key && crate::command_exists("claude");
            let hidden = s.hidden_agents.contains(&profile.id);
            let main = provider.map(|p| p.main_model.clone()).unwrap_or_default();
            let fast = provider.map(|p| p.fast_model.clone()).unwrap_or_default();
            let mut models = vec![main, fast];
            models.dedup();
            models.retain(|m| !m.is_empty());
            let custom_models: Vec<String> = s.custom_models.get(&profile.id).cloned().unwrap_or_default();
            serde_json::json!({
                "id": profile.id, "kind": "provider", "label": label,
                "available": available, "hidden": hidden, "models": models,
                "customModels": custom_models,
            })
        } else {
            let available = crate::command_exists(&profile.exe);
            let hidden = s.hidden_agents.contains(&profile.id);
            let models = if available { model_catalog::models_for(&profile.id, &base) } else { vec![] };
            let custom_models: Vec<String> = s.custom_models.get(&profile.id).cloned().unwrap_or_default();
            serde_json::json!({
                "id": profile.id, "kind": "builtin", "label": profile.id,
                "available": available, "hidden": hidden, "models": models,
                "customModels": custom_models,
            })
        }
    }).collect();

    ok_resp(serde_json::json!({ "agents": agents }))
}

async fn handle_coordinator_set(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let tile_id = v["tileId"].as_str().unwrap_or("").to_string();
    let on = v["on"].as_bool().unwrap_or(false);
    if tile_id.is_empty() {
        return err_resp("bad_request", "tileId required");
    }
    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let snapshot = {
        let mut guard = coordinators().lock().unwrap_or_else(|e| e.into_inner());
        if on { guard.insert(tile_id.clone()); } else { guard.remove(&tile_id); }
        guard.clone()
    };
    if !on {
        let runs_to_detach: Vec<String> = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs.values()
                .filter(|e| {
                    !e.run.status.is_terminal()
                        && e.run.coordinator_tile.as_deref() == Some(&tile_id)
                })
                .map(|e| e.run.id.clone())
                .collect()
        };
        for run_id in runs_to_detach {
            let run_id_c = run_id.clone();
            let now = unix_ms();
            let _ = commit(move |_| {
                Ok(Cmd::RunSetCoordinator { run_id: run_id_c, tile_id: None, now })
            }, run_id).await;
        }
    }
    crate::ui::broadcast_orch_changed("");
    if let Err(e) = save_coordinators(&base, &snapshot) {
        return err_resp("io", &e);
    }
    ok_resp(serde_json::json!({}))
}

fn handle_coordinator_list() -> String {
    let guard = coordinators().lock().unwrap_or_else(|e| e.into_inner());
    let tiles: Vec<&String> = guard.iter().collect();
    ok_resp(serde_json::json!({ "tiles": tiles }))
}

async fn handle_ai_commit_message(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let repo = match v["repo"].as_str().filter(|s| !s.is_empty()) {
        Some(r) => r.to_string(),
        None => return err_resp("bad_request", "repo required"),
    };
    let files = str_arr_field(&v, "files");

    let repo_c = repo.clone();
    let files_c = files.clone();
    let diff = match tokio::task::spawn_blocking(move || build_commit_diff(&repo_c, &files_c)).await {
        Ok(Ok(d)) => d,
        Ok(Err(e)) => return err_resp("git_error", &e),
        Err(_) => return err_resp("io", "spawn_blocking panic"),
    };

    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let s = settings::load(&base);
    let (provider, model) = ai::resolve_ai_target(&s, Some("commitMessage"));

    let prompt = format!(
        "Write a single-line semantic commit message for this diff.\n\
         Format: type: description\n\
         Rules: under 72 chars, English, no body, no quotes.\n\
         Return ONLY the commit message line, nothing else.\n\n\
         {diff}"
    );

    match tokio::task::spawn_blocking(move || ai::run_oneshot(ai::OneShotOpts { provider, model, prompt })).await {
        Ok(Ok(raw)) => {
            let message = ai::clean_commit_message(&raw);
            if message.is_empty() {
                return err_resp("bad_response", "AI returned empty commit message");
            }
            ok_resp(serde_json::json!({ "message": message }))
        }
        Ok(Err(e)) => err_from_str(&e),
        Err(_) => err_resp("io", "spawn_blocking panic"),
    }
}

fn build_commit_diff(repo: &str, files: &[String]) -> Result<String, String> {
    use crate::git_glance::git;

    let head_exists = git(repo, &["rev-parse", "--verify", "HEAD"]).is_ok();

    let mut parts: Vec<String> = Vec::new();

    let mut diff_args: Vec<String> = if head_exists {
        vec!["diff".into(), "HEAD".into(), "--".into()]
    } else {
        vec!["diff".into(), "--cached".into(), "4b825dc642cb6eb9a060e54bf8d69288fbee4904".into(), "--".into()]
    };
    diff_args.extend(files.iter().cloned());
    let diff_refs: Vec<&str> = diff_args.iter().map(|s| s.as_str()).collect();
    let diff_bytes = git(repo, &diff_refs).unwrap_or_default();
    let diff_str = String::from_utf8_lossy(&diff_bytes).into_owned();
    if !diff_str.is_empty() {
        parts.push(diff_str);
    }

    if !files.is_empty() {
        let mut ls_args: Vec<String> = vec![
            "ls-files".into(), "--others".into(), "--exclude-standard".into(), "--".into(),
        ];
        ls_args.extend(files.iter().cloned());
        let ls_refs: Vec<&str> = ls_args.iter().map(|s| s.as_str()).collect();
        let untracked_bytes = git(repo, &ls_refs).unwrap_or_default();
        let untracked = String::from_utf8_lossy(&untracked_bytes).into_owned();
        for path in untracked.lines().filter(|l| !l.trim().is_empty()) {
            let full_path = std::path::Path::new(repo).join(path);
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                parts.push(format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n{content}"));
            }
        }
    }

    let combined = parts.join("\n");
    const LIMIT: usize = 60_000;
    if combined.len() > LIMIT {
        let safe = (0..=LIMIT).rev().find(|&i| combined.is_char_boundary(i)).unwrap_or(0);
        let cut = combined[..safe].rfind('\n').unwrap_or(safe);
        Ok(format!("{}\n[diff truncated]", &combined[..cut]))
    } else {
        Ok(combined)
    }
}

async fn handle_ai_tidy(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let snapshot = match v["snapshot"].as_str().filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => return err_resp("bad_request", "snapshot required"),
    };

    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };
    let s = settings::load(&base);
    let (provider, model) = ai::resolve_ai_target(&s, Some("tidy"));

    let prompt = format!(
        "Given this workspace snapshot, return ONLY a JSON object with this exact shape:\n\
         {{\"projectOrder\":[\"<projectId>\",...],\"frames\":[{{\"title\":\"<short>\",\"frameId\":\"<existingId>\",\"path\":\"<projectPath>\",\"tileIds\":[\"<id>\",...]}}, ...]}}\n\
         Rules: order projects sensibly, group tiles by project then by topic/work area, never mix projects in one frame. \
         A frame with a path already represents that project: reuse it via frameId for that project's tiles instead of creating another. \
         Reuse other existing frames via frameId when they fit (omit frameId for new frames). \
         Set path to the projects[].path the frame belongs to, or omit it for frames outside any project. \
         Short titles (2-4 words), never invent tile, frame or project ids.\n\
         Return ONLY the JSON object, no prose.\n\n\
         {snapshot}"
    );

    match tokio::task::spawn_blocking(move || ai::run_oneshot(ai::OneShotOpts { provider, model, prompt })).await {
        Ok(Ok(raw)) => {
            match ai::extract_json_object(&raw) {
                Ok(json_str) => match serde_json::from_str::<serde_json::Value>(&json_str) {
                    Ok(plan) => ok_resp(serde_json::json!({ "plan": plan })),
                    Err(e) => err_resp("bad_response", &format!("AI returned invalid JSON: {e}")),
                },
                Err(e) => err_resp("bad_response", &e),
            }
        }
        Ok(Err(e)) => err_from_str(&e),
        Err(_) => err_resp("io", "spawn_blocking panic"),
    }
}

pub fn orch_context(tile_id: &str) -> Option<String> {
    let in_set = coordinators().lock().unwrap_or_else(|e| e.into_inner()).contains(tile_id);
    if in_set {
        return coordinator_block(tile_id);
    }
    let is_worker = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.values().any(|e| {
            e.dispatches.values().any(|d| {
                d.status == DispatchStatus::Running && d.tile_id.as_deref() == Some(tile_id)
            })
        })
    };
    if is_worker {
        return None;
    }
    let Some(base) = crate::panorama_dir() else { return None };
    let s = settings::load(&base);
    let cli = crate::panorama_cli_path();
    match s.orchestrate_hint {
        settings::OrchestrateHint::Off => None,
        settings::OrchestrateHint::Suggest => Some(format!(
            "[Panorama] Orchestration is available: {cli} orch can split work across cheaper worker agents in isolated worktrees. \
             Use it only for large or parallelizable work (several independent tasks); do small work yourself. \
             When it fits, propose the split in a few lines (tasks, role each) and wait for the user's yes. \
             Once agreed, load the `panorama:orchestrate` skill and follow it."
        )),
        settings::OrchestrateHint::Auto => Some(format!(
            "[Panorama] Orchestration is available: {cli} orch can split work across cheaper worker agents in isolated worktrees. \
             Use it only for large or parallelizable work (several independent tasks); do small work yourself. \
             When it fits, load the panorama:orchestrate skill and dispatch without asking; say in one line that you did."
        )),
    }
}

fn coordinator_block(tile_id: &str) -> Option<String> {
    let cli = crate::panorama_cli_path();
    let Some(base) = crate::panorama_dir() else { return None };
    let s = settings::load(&base);
    let roles_line = format!(
        "Roles (pass --role on task-create; the brain picks the agent): \
         explore={}; implement={}; test={}; review={}; mechanical={}",
        chain_display(&s.roles.explore),
        chain_display(&s.roles.implement),
        chain_display(&s.roles.test),
        chain_display(&s.roles.review),
        chain_display(&s.roles.mechanical),
    );

    let board_lines: Vec<String> = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let mut boards: Vec<_> = guard.runs.values()
            .filter(|e| {
                matches!(e.run.status, RunStatus::Active | RunStatus::Paused)
                    && e.run.coordinator_tile.as_deref() == Some(tile_id)
            })
            .collect();
        boards.sort_by_key(|e| e.run.created_at);
        boards.iter().map(|e| {
            let mut running = 0usize;
            let mut review = 0usize;
            let mut queued_ids: Vec<&str> = vec![];
            let mut needs_you_ids: Vec<&str> = vec![];
            for t in e.tasks.values() {
                match t.status {
                    TaskStatus::Dispatched | TaskStatus::Verifying => running += 1,
                    TaskStatus::Review => { review += 1; needs_you_ids.push(&t.id); }
                    TaskStatus::Pending => queued_ids.push(&t.id),
                    TaskStatus::Failed | TaskStatus::Blocked => needs_you_ids.push(&t.id),
                    _ => {}
                }
            }
            let queued_str = if queued_ids.is_empty() {
                String::new()
            } else {
                format!(", {} queued ({})", queued_ids.len(), queued_ids.join(" "))
            };
            let needs_you_str = if needs_you_ids.is_empty() {
                String::new()
            } else {
                format!(", needs you: {}", needs_you_ids.join(" "))
            };
            format!(
                "Board \"{}\" ({}): {} running, {} review{}{}",
                e.run.title, e.run.id, running, review, queued_str, needs_you_str
            )
        }).collect()
    };

    let boards_block = if board_lines.is_empty() {
        format!("No active boards yet. Create one with {cli} orch run-create, or the user may create boards from the UI.")
    } else {
        board_lines.join("\n")
    };

    Some(format!(
        "[Panorama] Orchestrate mode is ON in this terminal. You are the coordinator of a multi-agent run, not the implementer.\n\
         - If the `panorama:orchestrate` skill is not loaded in this conversation yet, load it with the Skill tool and follow it.\n\
         - Treat the user's message as work to split across workers. Do not edit project files yourself.\n\
         - When the request is clear, state the plan in a few lines and dispatch right away; ask first only if it is ambiguous.\n\
         - {boards_block}\n\
         - Tasks owned by the user (owner: user) are not yours to dispatch.\n\
         - Set --review on task-create only when the user asked to check results before the task counts as done.\n\
         - review status means the task is waiting for the user; do not complete it yourself unless the user tells you to.\n\
         - Read task details with {cli} orch run-show or {cli} orch task-list.\n\
         - {roles_line}. Change chains in Settings > Orchestration."
    ))
}

fn chain_display(chain: &[String]) -> String {
    if chain.is_empty() { return "(empty)".into(); }
    chain.join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::model::Limits;

    #[test]
    fn build_commit_diff_truncates_on_char_boundary() {
        let mut s = String::from("a");
        while s.len() < 60_010 {
            s.push('\u{20AC}');
        }
        const LIMIT: usize = 60_000;
        assert!(!s.is_char_boundary(LIMIT), "test requires mid-char boundary at LIMIT");
        let safe = (0..=LIMIT).rev().find(|&i| s.is_char_boundary(i)).unwrap_or(0);
        let _ = &s[..safe];
        assert!(safe < LIMIT);
    }

    fn setup_run_in_state(run_id: &str) {
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let cmd = Cmd::RunCreate {
            id: run_id.to_string(),
            title: "test".into(),
            repo: "/tmp".into(),
            base_ref: None,
            base_sha: None,
            setup: None,
            coordinator_tile: None,
            limits: Limits::default(),
            now: 1_000_000,
        };
        match guard.apply(&cmd) {
            ApplyResult::Ok(k, d) => guard.replay(&k, &d).expect("setup replay"),
            _ => {}
        }
    }

    fn inject_message(run_id: &str, to: &str) {
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.runs.get_mut(run_id) {
            entry.messages.push(Message {
                seq: entry.msg_seq + 1,
                from: "coordinator".into(),
                to: to.into(),
                msg_type: "note".into(),
                task_id: None,
                dispatch_id: None,
                reply_to: None,
                body: Some("wake".into()),
                at: 1_000_000,
                acked: false,
            });
            entry.msg_seq += 1;
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn long_poll_wakes_on_message() {
        let run_id = format!("r-lp-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis());
        setup_run_in_state(&run_id);

        let run_id_wait = run_id.clone();
        // peek=true: collect_ack_sync returns messages without writing ack to disk
        let check_fut = tokio::spawn(async move {
            let body = serde_json::json!({
                "runId": run_id_wait,
                "recipient": "coordinator",
                "wait": true,
                "peek": true,
                "timeoutMs": 5000
            });
            handle_check(body.to_string().as_bytes()).await
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        inject_message(&run_id, "coordinator");
        run_notify(&run_id).notify_waiters();

        let resp = tokio::time::timeout(Duration::from_secs(3), check_fut)
            .await
            .expect("check timed out in test")
            .expect("join");

        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        assert!(v["ok"].as_bool().unwrap_or(false), "check returned error: {resp}");
        let msgs = v["messages"].as_array().expect("messages field");
        assert!(!msgs.is_empty(), "expected at least one message");
    }

    fn no_sent() -> std::collections::HashSet<String> {
        std::collections::HashSet::new()
    }

    fn sent_with(reasons: &[&str]) -> std::collections::HashSet<String> {
        reasons.iter().map(|s| s.to_string()).collect()
    }

    fn base_facts() -> TickFacts {
        TickFacts {
            is_session_live: true,
            ever_live: true,
            consecutive_shell_ticks: 0,
            screen_quiet_secs: 0,
            agent_status: Some("busy".to_string()),
            running_secs: 10,
            is_waiting: false,
            limits_dispatch_timeout_min: 60,
            limits_stall_quiet_min: 5,
        }
    }

    fn mk_run_entry(run_id: &str) -> crate::orch::reducer::RunEntry {
        use crate::orch::reducer::RunEntry;
        let run = Run {
            id: run_id.to_string(),
            title: "Test Run".to_string(),
            repo: "/tmp/repo".to_string(),
            base_ref: None,
            base_sha: Some("abc123".to_string()),
            setup: None,
            coordinator_tile: None,
            status: RunStatus::Active,
            limits: Limits::default(),
            created_at: 1_000_000,
            updated_at: 1_000_000,
        };
        RunEntry {
            run,
            tasks: std::collections::HashMap::new(),
            dispatches: std::collections::HashMap::new(),
            messages: vec![],
            gates: std::collections::HashMap::new(),
            task_count: 0,
            gate_count: 0,
            msg_seq: 0,
            event_seq: 0,
            coordinator_cost_baseline: None,
            coordinator_cost_final: None,
        }
    }

    fn mk_task_entry(id: &str, status: TaskStatus, attempts: u32) -> Task {
        Task {
            id: id.to_string(),
            title: format!("Task {id}"),
            spec: "spec".to_string(),
            acceptance: None,
            deps: vec![],
            profile: None,
            escalation: vec![],
            verify: vec![],
            role: None,
            review: false,
            owner: None,
            status,
            result: None,
            integrated_sha: None,
            attempts,
            created_at: 1_000_000,
            updated_at: 1_000_000,
        }
    }

    fn mk_dispatch_entry(id: &str, task_id: &str, attempt: u32, retry_of: Option<&str>, cost_usd: Option<f64>, cost_source: Option<&str>, duration_ms: u64) -> Dispatch {
        Dispatch {
            id: id.to_string(),
            task_id: task_id.to_string(),
            attempt,
            retry_of: retry_of.map(str::to_string),
            profile: Some("claude".to_string()),
            model: None,
            tile_id: None,
            worktree: None,
            branch: None,
            status: DispatchStatus::Reported,
            outcome: None,
            head_sha: None,
            files_modified: vec![],
            summary: None,
            started_at: 1_000_000,
            ended_at: Some(1_000_000 + duration_ms),
            last_heartbeat: None,
            verifications: vec![],
            cost_usd,
            cost_source: cost_source.map(str::to_string),
        }
    }


    #[test]
    fn pick_escalation_no_list_uses_task_profile() {
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &[],
            attempt: 1,
            explicit_profile: None,
            explicit_model: None,
            task_profile: Some("opus"),
            latest_profile: None,
            latest_model: None,
        });
        assert_eq!(p, "opus");
        assert_eq!(m, None);
    }

    #[test]
    fn pick_escalation_no_list_uses_latest_dispatch_profile_and_model() {
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &[],
            attempt: 2,
            explicit_profile: None,
            explicit_model: None,
            task_profile: None,
            latest_profile: Some("haiku"),
            latest_model: Some("claude-haiku-4-5"),
        });
        assert_eq!(p, "haiku");
        assert_eq!(m.as_deref(), Some("claude-haiku-4-5"));
    }

    #[test]
    fn pick_escalation_list_shorter_than_attempts_reuses_last_entry() {
        let list = vec!["fast".to_string(), "opus".to_string()];
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &list,
            attempt: 5,
            explicit_profile: None,
            explicit_model: None,
            task_profile: None,
            latest_profile: None,
            latest_model: None,
        });
        assert_eq!(p, "opus");
        assert_eq!(m, None);
    }

    #[test]
    fn pick_escalation_profile_model_split_on_first_colon_slash_in_model() {
        let list = vec!["opencode:openai/gpt-5.5".to_string()];
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &list,
            attempt: 1,
            explicit_profile: None,
            explicit_model: None,
            task_profile: None,
            latest_profile: None,
            latest_model: None,
        });
        assert_eq!(p, "opencode");
        assert_eq!(m.as_deref(), Some("openai/gpt-5.5"));
    }

    #[test]
    fn pick_escalation_explicit_profile_and_model_override_list() {
        let list = vec!["fast".to_string()];
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &list,
            attempt: 1,
            explicit_profile: Some("opus"),
            explicit_model: Some("claude-opus-4-5"),
            task_profile: None,
            latest_profile: None,
            latest_model: None,
        });
        assert_eq!(p, "opus");
        assert_eq!(m.as_deref(), Some("claude-opus-4-5"));
    }

    #[test]
    fn pick_escalation_task_profile_with_model_split() {
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &[],
            attempt: 1,
            explicit_profile: None,
            explicit_model: None,
            task_profile: Some("claude:sonnet"),
            latest_profile: None,
            latest_model: None,
        });
        assert_eq!(p, "claude");
        assert_eq!(m.as_deref(), Some("sonnet"));
    }

    #[test]
    fn pick_escalation_explicit_model_only_overrides_list_model() {
        let list = vec!["fast:claude-haiku-3".to_string()];
        let (p, m) = pick_escalation(EscalationInput {
            escalation: &list,
            attempt: 1,
            explicit_profile: None,
            explicit_model: Some("claude-haiku-4-5"),
            task_profile: None,
            latest_profile: None,
            latest_model: None,
        });
        assert_eq!(p, "fast");
        assert_eq!(m.as_deref(), Some("claude-haiku-4-5"));
    }


    #[test]
    fn assess_session_gone_fires_when_no_live_session() {
        let mut facts = base_facts();
        facts.is_session_live = false;
        let findings = assess(&facts, &no_sent());
        assert_eq!(findings, vec![Finding::SessionGone]);
    }

    #[test]
    fn assess_session_gone_suppressed_when_already_sent() {
        let mut facts = base_facts();
        facts.is_session_live = false;
        let findings = assess(&facts, &sent_with(&["session_gone"]));
        assert!(findings.is_empty());
    }

    #[test]
    fn assess_agent_exited_needs_two_consecutive_shell_ticks() {
        let mut facts = base_facts();
        facts.consecutive_shell_ticks = 1;
        assert!(assess(&facts, &no_sent()).is_empty());
        facts.consecutive_shell_ticks = 2;
        let findings = assess(&facts, &no_sent());
        assert!(findings.contains(&Finding::AgentExited));
    }

    #[test]
    fn assess_agent_exited_once_per_reason() {
        let mut facts = base_facts();
        facts.consecutive_shell_ticks = 3;
        let findings = assess(&facts, &sent_with(&["agent_exited"]));
        assert!(!findings.contains(&Finding::AgentExited));
    }

    #[test]
    fn assess_timeout_fires_when_running_exceeds_limit() {
        let mut facts = base_facts();
        facts.running_secs = 61 * 60;
        let findings = assess(&facts, &no_sent());
        assert!(findings.contains(&Finding::Timeout));
    }

    #[test]
    fn assess_timeout_suppressed_when_already_sent() {
        let mut facts = base_facts();
        facts.running_secs = 61 * 60;
        let findings = assess(&facts, &sent_with(&["timeout"]));
        assert!(!findings.contains(&Finding::Timeout));
    }

    #[test]
    fn assess_quiet_fires_when_screen_stale_and_not_busy_and_not_waiting() {
        let mut facts = base_facts();
        facts.agent_status = None;
        facts.screen_quiet_secs = 5 * 60 + 1;
        let findings = assess(&facts, &no_sent());
        assert!(findings.contains(&Finding::Quiet));
    }

    #[test]
    fn assess_quiet_suppressed_when_agent_busy() {
        let mut facts = base_facts();
        facts.agent_status = Some("busy".to_string());
        facts.screen_quiet_secs = 5 * 60 + 1;
        let findings = assess(&facts, &no_sent());
        assert!(!findings.contains(&Finding::Quiet));
    }

    #[test]
    fn assess_quiet_suppressed_when_waiting() {
        let mut facts = base_facts();
        facts.agent_status = None;
        facts.is_waiting = true;
        facts.screen_quiet_secs = 5 * 60 + 1;
        let findings = assess(&facts, &no_sent());
        assert!(!findings.contains(&Finding::Quiet));
    }

    #[test]
    fn assess_quiet_suppressed_when_already_sent() {
        let mut facts = base_facts();
        facts.agent_status = None;
        facts.screen_quiet_secs = 5 * 60 + 1;
        let findings = assess(&facts, &sent_with(&["quiet"]));
        assert!(!findings.contains(&Finding::Quiet));
    }

    #[test]
    fn assess_grace_suppresses_session_gone_when_never_live_and_young() {
        let mut facts = base_facts();
        facts.is_session_live = false;
        facts.ever_live = false;
        facts.running_secs = 30;
        let findings = assess(&facts, &no_sent());
        assert!(findings.is_empty(), "must be suppressed during grace: {findings:?}");
    }

    #[test]
    fn assess_session_gone_fires_when_ever_live_regardless_of_age() {
        let mut facts = base_facts();
        facts.is_session_live = false;
        facts.ever_live = true;
        facts.running_secs = 10;
        let findings = assess(&facts, &no_sent());
        assert!(findings.contains(&Finding::SessionGone), "must fire when ever_live");
    }

    #[test]
    fn assess_session_gone_fires_when_old_even_if_never_live() {
        let mut facts = base_facts();
        facts.is_session_live = false;
        facts.ever_live = false;
        facts.running_secs = SESSION_GRACE_SECS + 1;
        let findings = assess(&facts, &no_sent());
        assert!(findings.contains(&Finding::SessionGone), "must fire after grace expires");
    }

    #[test]
    fn build_report_totals_exact_vs_unknown_cost() {
        let mut entry = mk_run_entry("r1");
        let mut t1 = mk_task_entry("t1", TaskStatus::Completed, 1);
        t1.integrated_sha = Some("sha1".to_string());
        entry.tasks.insert("t1".to_string(), t1);
        entry.dispatches.insert("r1.t1.1".to_string(),
            mk_dispatch_entry("r1.t1.1", "t1", 1, None, Some(0.05), Some("exact"), 60_000));
        entry.dispatches.insert("r1.t1.2".to_string(),
            mk_dispatch_entry("r1.t1.2", "t1", 2, Some("r1.t1.1"), None, Some("unknown"), 30_000));

        let report = build_report(&entry, None, "json");
        let totals = &report["totals"];
        assert_eq!(totals["unknownCostCount"].as_u64(), Some(1));
        assert!((totals["totalExactCostUsd"].as_f64().unwrap() - 0.05).abs() < 1e-9);
        assert_eq!(totals["retries"].as_u64(), Some(1));
    }

    #[test]
    fn build_report_totals_cost_per_completed_task() {
        let mut entry = mk_run_entry("r1");
        let t1 = mk_task_entry("t1", TaskStatus::Completed, 1);
        let t2 = mk_task_entry("t2", TaskStatus::Completed, 1);
        entry.tasks.insert("t1".to_string(), t1);
        entry.tasks.insert("t2".to_string(), t2);
        entry.dispatches.insert("r1.t1.1".to_string(),
            mk_dispatch_entry("r1.t1.1", "t1", 1, None, Some(0.10), Some("exact"), 10_000));
        entry.dispatches.insert("r1.t2.1".to_string(),
            mk_dispatch_entry("r1.t2.1", "t2", 1, None, Some(0.20), Some("exact"), 10_000));

        let report = build_report(&entry, None, "json");
        let totals = &report["totals"];
        let per_task = totals["costPerCompletedTask"].as_f64().unwrap();
        assert!((per_task - 0.15).abs() < 1e-9);
        assert!((totals["totalExactCostUsd"].as_f64().unwrap() - 0.30).abs() < 1e-9);
    }

    #[test]
    fn build_report_totals_retries_count() {
        let mut entry = mk_run_entry("r1");
        let t1 = mk_task_entry("t1", TaskStatus::Completed, 2);
        entry.tasks.insert("t1".to_string(), t1);
        entry.dispatches.insert("r1.t1.1".to_string(),
            mk_dispatch_entry("r1.t1.1", "t1", 1, None, Some(0.05), Some("exact"), 10_000));
        entry.dispatches.insert("r1.t1.2".to_string(),
            mk_dispatch_entry("r1.t1.2", "t1", 2, Some("r1.t1.1"), Some(0.07), Some("exact"), 10_000));

        let report = build_report(&entry, None, "json");
        let totals = &report["totals"];
        assert_eq!(totals["retries"].as_u64(), Some(1));
        assert_eq!(totals["dispatches"].as_u64(), Some(2));
    }


    #[test]
    fn build_report_includes_conflict_files_for_blocked_tasks() {
        let mut entry = mk_run_entry("r1");
        let mut t1 = mk_task_entry("t1", TaskStatus::Blocked, 1);
        t1.result = Some("src/foo.ts, src/bar.ts".to_string());
        entry.tasks.insert("t1".to_string(), t1);

        let report = build_report(&entry, None, "json");
        let tasks = report["tasks"].as_array().unwrap();
        let task = tasks.iter().find(|t| t["id"].as_str() == Some("t1")).unwrap();
        let files = task["conflictFiles"].as_array().unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].as_str(), Some("src/foo.ts"));
        assert_eq!(files[1].as_str(), Some("src/bar.ts"));
    }

    #[test]
    fn build_report_conflict_files_empty_for_non_blocked_tasks() {
        let mut entry = mk_run_entry("r1");
        let t1 = mk_task_entry("t1", TaskStatus::Completed, 1);
        entry.tasks.insert("t1".to_string(), t1);

        let report = build_report(&entry, None, "json");
        let tasks = report["tasks"].as_array().unwrap();
        let task = tasks.iter().find(|t| t["id"].as_str() == Some("t1")).unwrap();
        let files = task["conflictFiles"].as_array().unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn build_report_markdown_includes_conflict_files() {
        let mut entry = mk_run_entry("r1");
        let mut t1 = mk_task_entry("t1", TaskStatus::Blocked, 1);
        t1.result = Some("src/a.ts, src/b.ts".to_string());
        entry.tasks.insert("t1".to_string(), t1);

        let report = build_report(&entry, None, "markdown");
        let md = report["text"].as_str().unwrap();
        assert!(md.contains("Conflict files:"), "markdown missing conflict files section");
        assert!(md.contains("src/a.ts"), "markdown missing conflict file name");
    }

    #[test]
    fn task_cancel_running_stops_dispatch_then_cancels() {
        let run_id = format!("r-cancel-run-{}", unix_ms());
        let task_id = "t1".to_string();
        let dispatch_id = format!("{run_id}.t1.1");
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let cmd = Cmd::RunCreate {
            id: run_id.clone(), title: "test".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits::default(), now: 1_000_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let cmd = Cmd::TaskCreate {
            run_id: run_id.clone(), id: task_id.clone(), title: "T1".into(),
            spec: "spec".into(), acceptance: None, deps: vec![], profile: None,
            escalation: vec![], verify: vec![], role: None, review: false, owner: None, now: 1_000_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let cmd = Cmd::Dispatch {
            run_id: run_id.clone(), id: dispatch_id.clone(), task_id: task_id.clone(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: 1_000_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let cmd = Cmd::DispatchUpdate {
            run_id: run_id.clone(), dispatch_id: dispatch_id.clone(),
            status: DispatchStatus::Stopped, summary: None, now: 1_001_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let entry = guard.runs.get(&run_id).unwrap();
        assert_eq!(entry.tasks.get("t1").unwrap().status, TaskStatus::Failed, "task->failed after dispatch stop");
        assert_eq!(entry.dispatches.get(&dispatch_id).unwrap().status, DispatchStatus::Stopped);
        let cmd = Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: task_id.clone(),
            status: Some(TaskStatus::Cancelled), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: 1_002_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let entry = guard.runs.get(&run_id).unwrap();
        assert_eq!(entry.tasks.get("t1").unwrap().status, TaskStatus::Cancelled);
        let seq = entry.next_msg_seq();
        let cmd = Cmd::Send {
            run_id: run_id.clone(), seq, from: "user".to_string(),
            to: vec!["coordinator".to_string()], msg_type: "note".to_string(),
            task_id: Some(task_id.clone()), dispatch_id: None, reply_to: None,
            body: Some("User cancelled task t1 (T1) from the run board. Do not retry it unless the user asks.".into()),
            outcome: None, files_modified: vec![], summary: None, now: 1_003_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let entry = guard.runs.get(&run_id).unwrap();
        let has_msg = entry.messages.iter().any(|m| m.to == "coordinator" && m.from == "user" && !m.acked);
        assert!(has_msg, "coordinator message must be present and unacked");
    }

    #[test]
    fn task_cancel_pending_cancels_and_posts_message() {
        let run_id = format!("r-cancel-pend-{}", unix_ms());
        let task_id = "t1".to_string();
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let cmd = Cmd::RunCreate {
            id: run_id.clone(), title: "test".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits::default(), now: 1_000_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let cmd = Cmd::TaskCreate {
            run_id: run_id.clone(), id: task_id.clone(), title: "T1".into(),
            spec: "spec".into(), acceptance: None, deps: vec![], profile: None,
            escalation: vec![], verify: vec![], role: None, review: false, owner: None, now: 1_000_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let cmd = Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: task_id.clone(),
            status: Some(TaskStatus::Cancelled), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: 1_001_000,
        };
        match guard.apply(&cmd) {
            ApplyResult::Ok(k, d) => guard.replay(&k, &d).unwrap(),
            ApplyResult::Err(e) => panic!("expected Ok, got Err: {}", e.message()),
            ApplyResult::Stale(..) => panic!("expected Ok, got Stale"),
        }
        assert_eq!(guard.runs.get(&run_id).unwrap().tasks.get("t1").unwrap().status, TaskStatus::Cancelled);
        let entry = guard.runs.get(&run_id).unwrap();
        let seq = entry.next_msg_seq();
        let cmd = Cmd::Send {
            run_id: run_id.clone(), seq, from: "user".to_string(),
            to: vec!["coordinator".to_string()], msg_type: "note".to_string(),
            task_id: Some(task_id.clone()), dispatch_id: None, reply_to: None,
            body: Some("User cancelled task t1".into()),
            outcome: None, files_modified: vec![], summary: None, now: 1_002_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        let has_msg = guard.runs.get(&run_id).unwrap().messages.iter()
            .any(|m| m.to == "coordinator" && m.from == "user");
        assert!(has_msg);
    }

    #[test]
    fn task_cancel_completed_returns_conflict() {
        let run_id = format!("r-cancel-done-{}", unix_ms());
        let task_id = "t1".to_string();
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let cmd = Cmd::RunCreate {
            id: run_id.clone(), title: "test".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits::default(), now: 1_000_000,
        };
        if let ApplyResult::Ok(k, d) = guard.apply(&cmd) { guard.replay(&k, &d).unwrap(); }
        if let Some(entry) = guard.runs.get_mut(&run_id) {
            let mut task = mk_task_entry(&task_id, TaskStatus::Completed, 1);
            task.status = TaskStatus::Completed;
            entry.tasks.insert(task_id.clone(), task);
        }
        let cmd = Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: task_id.clone(),
            status: Some(TaskStatus::Cancelled), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: 1_001_000,
        };
        match guard.apply(&cmd) {
            ApplyResult::Err(e) => assert!(
                e.message().contains("terminal") || e.message().contains("invalid task status"),
                "unexpected error: {}", e.message()
            ),
            ApplyResult::Ok(..) => panic!("expected Err for cancelling completed task, got Ok"),
            ApplyResult::Stale(..) => panic!("expected Err for cancelling completed task, got Stale"),
        }
    }

    #[test]
    fn settings_defaults_when_file_missing() {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("orch-mod-defaults-{ns}"));
        std::fs::create_dir_all(&dir).unwrap();
        let s = settings::load(&dir);
        assert_eq!(s.coordinator, "claude:opus");
        assert_eq!(s.roles.implement, vec!["claude:haiku", "claude:sonnet"]);
        assert!(s.providers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn tmp_dir(label: &str) -> std::path::PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("orch-f8-{label}-{ns}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn coordinator_persist_load_roundtrip() {
        let dir = tmp_dir("coord-rt");
        let mut set = HashSet::new();
        set.insert("tile-abc".to_string());
        set.insert("tile-xyz".to_string());
        save_coordinators(&dir, &set).unwrap();
        let loaded = load_coordinators(&dir);
        assert_eq!(loaded, set);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn orch_context_no_coordinator_block_when_not_in_set() {
        let tile = "tile-not-in-set-99881";
        let result = orch_context(tile);
        if let Some(ctx) = result {
            assert!(!ctx.contains("Orchestrate mode is ON"),
                "non-coordinator tile must not get the coordinator block: {ctx}");
        }
    }

    #[test]
    fn orch_context_coordinator_no_active_run() {
        let tile = format!("tile-coord-norun-{}", unix_ms());
        coordinators().lock().unwrap_or_else(|e| e.into_inner()).insert(tile.clone());
        let result = orch_context(&tile);
        if let Some(ctx) = result {
            assert!(ctx.contains("Orchestrate mode is ON"), "missing header: {ctx}");
            assert!(ctx.contains("No active boards yet"), "expected no-boards line: {ctx}");
            assert!(ctx.contains("Roles"), "missing roles line: {ctx}");
        }
    }

    #[test]
    fn orch_context_coordinator_with_active_run() {
        let tile = format!("tile-coord-run-{}", unix_ms());
        let run_id = format!("r-ctx-{}", unix_ms());
        coordinators().lock().unwrap_or_else(|e| e.into_inner()).insert(tile.clone());
        {
            let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let cmd = Cmd::RunCreate {
                id: run_id.clone(),
                title: "F8 test run".into(),
                repo: "/tmp".into(),
                base_ref: None,
                base_sha: None,
                setup: None,
                coordinator_tile: Some(tile.clone()),
                limits: Limits::default(),
                now: unix_ms(),
            };
            match guard.apply(&cmd) {
                ApplyResult::Ok(k, d) => guard.replay(&k, &d).expect("replay"),
                _ => {}
            }
        }
        let result = orch_context(&tile);
        if let Some(ctx) = result {
            assert!(ctx.contains("Orchestrate mode is ON"), "missing header: {ctx}");
            assert!(ctx.contains(&run_id), "missing run id: {ctx}");
            assert!(ctx.contains("F8 test run"), "missing run title: {ctx}");
            assert!(ctx.contains("running"), "missing board summary: {ctx}");
        }
    }

    #[test]
    fn run_list_open_gates_count() {
        let run_id = format!("r-gates-{}", unix_ms());
        setup_run_in_state(&run_id);
        {
            let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
            if let Some(entry) = guard.runs.get_mut(&run_id) {
                entry.gates.insert("g1".to_string(), crate::orch::model::Gate {
                    id: "g1".to_string(),
                    task_id: "t1".to_string(),
                    question: "Should I proceed?".into(),
                    options: vec!["yes".into(), "no".into()],
                    resolution: None,
                    created_at: 0,
                    resolved_at: None,
                });
                entry.gates.insert("g2".to_string(), crate::orch::model::Gate {
                    id: "g2".to_string(),
                    task_id: "t1".to_string(),
                    question: "Another question?".into(),
                    options: vec!["a".into()],
                    resolution: Some("a".into()),
                    created_at: 0,
                    resolved_at: Some(1),
                });
            }
        }
        let resp = handle_run_list();
        let v: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let run_row = v["runs"].as_array().unwrap()
            .iter()
            .find(|r| r["run"]["id"].as_str() == Some(&run_id))
            .expect("run not found in list");
        assert_eq!(run_row["openGates"].as_u64(), Some(1), "expected 1 open gate");
    }

    #[test]
    fn orch_context_worker_tile_returns_none() {
        let run_id = format!("r-worker-ctx-{}", unix_ms());
        let tile = format!("tile-worker-{}", unix_ms());
        {
            let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let cmd = Cmd::RunCreate {
                id: run_id.clone(), title: "run".into(), repo: "/tmp".into(),
                base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
                limits: Limits::default(), now: unix_ms(),
            };
            match guard.apply(&cmd) {
                ApplyResult::Ok(k, d) => guard.replay(&k, &d).expect("replay"),
                _ => {}
            }
            let cmd = Cmd::TaskCreate {
                run_id: run_id.clone(), id: "t1".into(), title: "t".into(),
                spec: "s".into(), acceptance: None, deps: vec![], profile: None,
                escalation: vec![], verify: vec![], role: None, review: false,
                owner: None, now: unix_ms(),
            };
            match guard.apply(&cmd) {
                ApplyResult::Ok(k, d) => guard.replay(&k, &d).expect("replay"),
                _ => {}
            }
            let cmd = Cmd::Dispatch {
                run_id: run_id.clone(), id: format!("{run_id}.t1.1"),
                task_id: "t1".into(), attempt: 1, retry_of: None,
                profile: None, model: None, tile_id: Some(tile.clone()),
                owner: None, now: unix_ms(),
            };
            match guard.apply(&cmd) {
                ApplyResult::Ok(k, d) => guard.replay(&k, &d).expect("replay"),
                _ => {}
            }
        }
        let result = orch_context(&tile);
        assert!(result.is_none(), "worker tile should get None from orch_context");
    }

    #[test]
    fn cost_aggregates_by_role_and_model() {
        use crate::orch::reducer::RunEntry;
        use crate::orch::model::*;

        let run = Run {
            id: "r1".into(), title: "test".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None,
            coordinator_tile: None, status: RunStatus::Active,
            limits: Limits::default(), created_at: 0, updated_at: 0,
        };
        let mut entry = RunEntry::new(run);
        let t1 = Task {
            id: "t1".into(), title: "t1".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec![], verify: vec![],
            role: Some(Role::Explore), review: false, owner: None,
            status: TaskStatus::Completed, result: None, integrated_sha: None,
            attempts: 1, created_at: 0, updated_at: 0,
        };
        let mut t2 = t1.clone();
        t2.id = "t2".into();
        t2.role = Some(Role::Implement);
        entry.tasks.insert("t1".into(), t1);
        entry.tasks.insert("t2".into(), t2);
        let d1 = Dispatch {
            id: "d1".into(), task_id: "t1".into(), attempt: 1, retry_of: None,
            profile: Some("claude".into()), model: Some("haiku".into()),
            tile_id: None, worktree: None, branch: None, status: DispatchStatus::Reported,
            outcome: None, head_sha: None, files_modified: vec![], summary: None,
            started_at: 0, ended_at: None, last_heartbeat: None, verifications: vec![],
            cost_usd: Some(0.1), cost_source: Some("exact".into()),
        };
        let mut d2 = d1.clone();
        d2.id = "d2".into();
        d2.task_id = "t2".into();
        d2.model = Some("sonnet".into());
        d2.cost_usd = None;
        let mut d3 = d1.clone();
        d3.id = "d3".into();
        d3.task_id = "t1".into();
        d3.cost_usd = Some(0.05);
        entry.dispatches.insert("d1".into(), d1);
        entry.dispatches.insert("d2".into(), d2);
        entry.dispatches.insert("d3".into(), d3);

        let (by_role, by_model) = cost_aggregates(&entry);
        let explore = &by_role["explore"];
        assert_eq!(explore["dispatches"].as_u64(), Some(2), "explore dispatches");
        assert_eq!(explore["reported"].as_u64(), Some(2), "explore reported");
        let usd = explore["usd"].as_f64().unwrap();
        assert!((usd - 0.15).abs() < 1e-9, "explore usd = {usd}");
        let implement = &by_role["implement"];
        assert_eq!(implement["dispatches"].as_u64(), Some(1));
        assert_eq!(implement["reported"].as_u64(), Some(0));
        let sonnet = &by_model["claude:sonnet"];
        assert_eq!(sonnet["dispatches"].as_u64(), Some(1));
        assert_eq!(sonnet["reported"].as_u64(), Some(0));
    }
}
