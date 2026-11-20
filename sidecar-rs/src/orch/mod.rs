mod model;
mod reducer;
mod store;
pub mod cli;
pub mod exec;
pub mod git;
pub mod profiles;
pub mod worker;

use std::collections::HashMap;
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

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

fn notifies() -> &'static Mutex<HashMap<String, Arc<tokio::sync::Notify>>> {
    NOTIFIES.get_or_init(|| Mutex::new(HashMap::new()))
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
        "run-list" => handle_run_list(),
        "run-show" => handle_run_show(body),
        "run-set-status" => handle_run_set_status(body).await,
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
        "worker-stop" => handle_worker_stop(body).await,
        "worker-release" => handle_worker_release(body).await,
        "integrate" => handle_integrate(body).await,
        "worktree-clean" => handle_worktree_clean(body).await,
        "verify" => handle_verify(body).await,
        "retry" => handle_retry(body).await,
        "report" => handle_report(body).await,
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
                if dtile == tile_id && dispatch.status == DispatchStatus::Running {
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
                if dtile == tile_id && dispatch.status == DispatchStatus::Running {
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
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let run = guard.runs.get(&id).map(|e| serde_json::to_value(&e.run).unwrap_or_default());
            ok_resp(serde_json::json!({"run": run}))
        }
        CommitResult::Stale => err_resp("stale", "run create stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

fn handle_run_list() -> String {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let runs: Vec<Value> = guard.runs.values().map(|e| {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for t in e.tasks.values() {
            *counts.entry(format!("{:?}", t.status).to_lowercase()).or_insert(0) += 1;
        }
        serde_json::json!({"run": e.run, "taskCounts": counts})
    }).collect();
    ok_resp(serde_json::json!({"runs": runs}))
}

fn handle_run_show(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
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
    let dispatches: Vec<&Dispatch> = entry.dispatches.values().collect();
    let gates: Vec<&Gate> = entry.gates.values().collect();
    ok_resp(serde_json::json!({
        "run": entry.run,
        "tasks": tasks,
        "dispatches": dispatches,
        "gates": gates
    }))
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
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::RunSetStatus { run_id: run_id_c, status, now })
    }, run_id).await;
    match result {
        CommitResult::Ok(_) => ok_resp(serde_json::json!({})),
        CommitResult::Stale => err_resp("stale", "run set-status stale"),
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
    let now = unix_ms();

    let deps = match strict_str_arr(&v, "deps") { Ok(a) => a, Err(e) => return e };
    let escalation = match strict_str_arr(&v, "escalation") { Ok(a) => a, Err(e) => return e };
    let verify = match strict_str_arr(&v, "verify") { Ok(a) => a, Err(e) => return e };

    let run_id_c = run_id.clone();
    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let task_id = entry.next_task_id();
        Ok(Cmd::TaskCreate {
            run_id: run_id_c, id: task_id,
            title, spec, acceptance, deps, profile, escalation, verify, review, now,
        })
    }, run_id.clone()).await;

    match result {
        CommitResult::Ok(data) => {
            let task_id = data["id"].as_str().unwrap_or("").to_string();
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let task = guard.runs.get(&run_id)
                .and_then(|e| e.tasks.get(&task_id))
                .map(|t| serde_json::to_value(t).unwrap_or_default());
            ok_resp(serde_json::json!({"task": task}))
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
    let result = v["result"].as_str().map(str::to_string);
    let spec = v["spec"].as_str().map(str::to_string);
    let acceptance = v["acceptance"].as_str().map(str::to_string);
    let profile = v["profile"].as_str().map(str::to_string);
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let commit_result = commit(move |_| {
        Ok(Cmd::TaskUpdate {
            run_id: run_id_c, task_id: task_id_c, status, result,
            spec, acceptance, verify, profile, escalation, now,
        })
    }, run_id).await;
    match commit_result {
        CommitResult::Ok(_) => ok_resp(serde_json::json!({})),
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
            attempt, retry_of, profile, model, tile_id, now,
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
            body: body_str, outcome, files_modified, summary, now,
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
        CommitResult::Stale => err_resp("stale", "report from a superseded dispatch"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

// Runs before worker_done is recorded, so a coordinator that integrates or cleans as soon as it
// reads the message always sees the worker's changes committed on the task branch.
async fn commit_worker_leftovers(run_id: &str, dispatch_id: &str) {
    let target = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(run_id).and_then(|entry| {
            let d = entry.dispatches.get(dispatch_id)?;
            if d.status != DispatchStatus::Running {
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
    let (wt_path, base_sha, commit_msg, task_id, tile_id) = {
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
        let title = entry.tasks.get(&task_id).map(|t| t.title.clone()).unwrap_or_default();
        (wt, entry.run.base_sha.clone(), format!("orch({task_id}): {title}"), task_id, tile_id)
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

    let (cost_usd, cost_source) = tile_id
        .as_deref()
        .and_then(|tid| crate::sessions().lock().unwrap().get(tid).cloned())
        .and_then(|s| crate::cached_agent_state(&s))
        .and_then(|js| serde_json::from_str::<serde_json::Value>(&js).ok())
        .and_then(|v| v["costUsd"].as_f64())
        .map(|c| (Some(c), "exact".to_string()))
        .unwrap_or((None, "unknown".to_string()));

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
        let p = task_profile.or(latest_profile).unwrap_or("").to_string();
        (p, explicit_model.map(str::to_string).or_else(|| latest_model.map(str::to_string)))
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

pub(crate) struct TickFacts {
    pub is_session_live: bool,
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
        if !sent.contains("session_gone") {
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

    let running_dispatches: Vec<(String, String, String, u64, Limits)> = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.values()
            .filter(|e| !e.run.status.is_terminal())
            .flat_map(|e| {
                let limits = e.run.limits.clone();
                e.dispatches.values()
                    .filter(|d| d.status == DispatchStatus::Running)
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

        let (consecutive_shell_ticks, screen_quiet_secs, screen_text) = {
            let mut watch = supervision_watch().lock().unwrap_or_else(|e| e.into_inner());
            let entry = watch.entry(dispatch_id.clone()).or_insert_with(|| DispatchWatch {
                consecutive_shell_ticks: 0,
                last_screen: String::new(),
                screen_last_changed_secs: now_secs,
                sent: std::collections::HashSet::new(),
            });
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
            (entry.consecutive_shell_ticks, quiet, screen)
        };

        let running_secs = now_secs.saturating_sub(started_secs);

        let sent = {
            let watch = supervision_watch().lock().unwrap_or_else(|e| e.into_inner());
            watch.get(&dispatch_id).map(|e| e.sent.clone()).unwrap_or_default()
        };

        let facts = TickFacts {
            is_session_live,
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
                    let summary = if finding == Finding::AgentExited {
                        let tail = screen_text.lines().rev().take(10).collect::<Vec<_>>()
                            .into_iter().rev().collect::<Vec<_>>().join("\n");
                        format!("agent exited\n{tail}")
                    } else {
                        "worker session ended".to_string()
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
                            body: Some(summary),
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

async fn handle_integrate(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }

    let (repo, branch, base_sha) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        if !entry.tasks.contains_key(&task_id) {
            return err_resp("not_found", "task not found");
        }
        let latest_dispatch = entry.dispatches.values()
            .filter(|d| d.task_id == task_id)
            .max_by_key(|d| d.attempt);
        let branch = match latest_dispatch.and_then(|d| d.branch.clone()) {
            Some(b) => b,
            None => return err_resp("conflict", "no worktree branch for task"),
        };
        (entry.run.repo.clone(), branch, entry.run.base_sha.clone())
    };

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
            let now = unix_ms();
            let seq = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                guard.runs.get(&run_id).map(|e| e.next_msg_seq()).unwrap_or(0)
            };
            let result = commit(move |_| {
                Ok(Cmd::TaskIntegrate {
                    run_id: run_id_c,
                    task_id: task_id_c,
                    integrated_sha: sha.clone(),
                    msg_seq: seq,
                    now,
                })
            }, run_id.clone()).await;
            match result {
                CommitResult::Ok(_) => ok_resp(serde_json::json!({"integrated": true})),
                CommitResult::Stale => err_resp("stale", "integrate stale"),
                CommitResult::Err(e) => err_from_str(&e),
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

    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };

    let profiles = load_profiles(&base);
    let Some(profile) = find_profile(&profiles, &profile_id) else {
        return err_resp("bad_request", &format!("unknown profile: {profile_id}"));
    };
    let profile = profile.clone();

    let tile_id = crate::ui::new_tile_id();
    let cli_path = crate::panorama_cli_path();
    let now = unix_ms();

    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let tile_id_c = tile_id.clone();
    let profile_id_c = profile_id.clone();
    let model_c = model.clone();
    let retry_of_c = retry_of.clone();

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

    let (task_clone, dispatch_clone, deps_results, base_cwd, run_base_sha, run_setup, task_has_deps) = {
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
        (task, dispatch, deps_results, cwd, entry.run.base_sha.clone(), entry.run.setup.clone(), has_deps)
    };

    let use_worktree = worktree_mode != "none" && run_base_sha.is_some();
    let cwd = if use_worktree {
        let repo = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs.get(&run_id).map(|e| e.run.repo.clone()).unwrap_or_default()
        };

        let base_sha_val = run_base_sha.clone().unwrap_or_default();
        let int_branch = git::integration_branch(&run_id);
        let repo_c2 = repo.clone();
        let int_br_c = int_branch.clone();
        let base_for_wt = tokio::task::spawn_blocking(move || {
            if task_has_deps {
                git::rev_parse(&repo_c2, &int_br_c).unwrap_or(base_sha_val)
            } else {
                base_sha_val
            }
        }).await.unwrap_or_else(|_| run_base_sha.clone().unwrap_or_default());

        let wt_path = git::worktree_path(&repo, &run_id, &task_id, dispatch_clone.attempt);
        let wt_branch = git::worktree_branch(&run_id, &task_id, dispatch_clone.attempt);

        let repo_c3 = repo.clone();
        let wt_path_c = wt_path.clone();
        let wt_branch_c = wt_branch.clone();
        let base_c = base_for_wt.clone();
        let wt_result = tokio::task::spawn_blocking(move || {
            git::worktree_add(&repo_c3, &wt_path_c, &wt_branch_c, &base_c)
        }).await;

        match wt_result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let run_id2 = run_id.clone();
                let dispatch_id2 = dispatch_id.clone();
                let now2 = unix_ms();
                let e2 = e.clone();
                commit(move |_| {
                    Ok(Cmd::DispatchUpdate {
                        run_id: run_id2, dispatch_id: dispatch_id2,
                        status: DispatchStatus::Failed,
                        summary: Some(format!("worktree create failed: {e2}")),
                        now: now2,
                    })
                }, run_id.clone()).await;
                return err_resp("conflict", &format!("worktree create failed: {e}"));
            }
            Err(e) => {
                eprintln!("[orch] spawn_blocking panic in worktree_add: {e}");
                return err_resp("io", "worktree create: spawn failed");
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

        if let Some(ref setup_cmd) = run_setup {
            use crate::orch::exec::{RunToExitInput, run_to_exit};
            let setup_input = RunToExitInput {
                cwd: wt_path.clone(),
                cmd: setup_cmd.clone(),
                timeout: std::time::Duration::from_secs(600),
                dispatch_id: dispatch_id.clone(),
                purpose: "setup".into(),
            };
            match run_to_exit(setup_input).await {
                Ok(outcome) if !outcome.timed_out && outcome.exit_code == Some(0) => {}
                Ok(outcome) => {
                    let tail = outcome.tail.join("\n");
                    let summary = if outcome.timed_out {
                        format!("setup timed out\n{tail}")
                    } else {
                        format!("setup exited {:?}\n{tail}", outcome.exit_code)
                    };
                    let run_id2 = run_id.clone();
                    let dispatch_id2 = dispatch_id.clone();
                    let now2 = unix_ms();
                    let summary2 = summary.clone();
                    commit(move |_| {
                        Ok(Cmd::DispatchUpdate {
                            run_id: run_id2, dispatch_id: dispatch_id2,
                            status: DispatchStatus::Failed,
                            summary: Some(summary2),
                            now: now2,
                        })
                    }, run_id.clone()).await;
                    return err_resp("conflict", &format!("setup failed: {summary}"));
                }
                Err(e) => {
                    let run_id2 = run_id.clone();
                    let dispatch_id2 = dispatch_id.clone();
                    let now2 = unix_ms();
                    let e2 = e.clone();
                    commit(move |_| {
                        Ok(Cmd::DispatchUpdate {
                            run_id: run_id2, dispatch_id: dispatch_id2,
                            status: DispatchStatus::Failed,
                            summary: Some(format!("setup start failed: {e2}")),
                            now: now2,
                        })
                    }, run_id.clone()).await;
                    return err_resp("io", &format!("setup start failed: {e}"));
                }
            }
        }

        wt_path
    } else {
        base_cwd
    };

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
            let run_id2 = run_id.clone();
            let dispatch_id2 = dispatch_id.clone();
            let now2 = unix_ms();
            let e2 = e.clone();
            commit(move |_| {
                Ok(Cmd::DispatchUpdate {
                    run_id: run_id2, dispatch_id: dispatch_id2,
                    status: DispatchStatus::Failed,
                    summary: Some(format!("brief write failed: {e2}")),
                    now: now2,
                })
            }, run_id.clone()).await;
            return err_resp("io", &format!("brief write failed: {e}"));
        }
    };

    let extra_env: Vec<(String, String)> = {
        let mut env = resolve_env(&profile.env);
        env.push(("PANORAMA_RUN_ID".into(), run_id.clone()));
        env.push(("PANORAMA_DISPATCH_ID".into(), dispatch_id.clone()));
        env
    };

    let run_title = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id).map(|e| e.run.title.clone()).unwrap_or_default()
    };
    let tile_id2 = tile_id.clone();
    let profile2 = profile.clone();
    let model2 = model.clone();
    let run_id2 = run_id.clone();
    let dispatch_id2 = dispatch_id.clone();
    let cwd2 = cwd.clone();

    tokio::spawn(async move {
        let launch = WorkerLaunch {
            tile_id: &tile_id2,
            cwd: &cwd2,
            group: TileGroup { id: &run_id2, title: &run_title },
            profile: &profile2,
            model: model2.as_deref(),
            extra_env,
            brief_path: &brief_path,
        };
        if let Err(e) = spawn_worker(launch).await {
            let now3 = unix_ms();
            let run_id_arg = run_id2.clone();
            commit(move |_| {
                Ok(Cmd::DispatchUpdate {
                    run_id: run_id2.clone(), dispatch_id: dispatch_id2.clone(),
                    status: DispatchStatus::Failed,
                    summary: Some(format!("spawn failed: {e}")),
                    now: now3,
                })
            }, run_id_arg).await;
        }
    });

    let dispatch_json = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.dispatches.get(&dispatch_id))
            .and_then(|d| serde_json::to_value(d).ok())
            .unwrap_or(serde_json::json!({"id": dispatch_id}))
    };
    ok_resp(serde_json::json!({"dispatch": dispatch_json}))
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

async fn handle_worker_stop(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || dispatch_id.is_empty() {
        return err_resp("bad_request", "runId and dispatchId required");
    }
    let now = unix_ms();
    let tile_id = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.dispatches.get(&dispatch_id))
            .and_then(|d| d.tile_id.clone())
            .unwrap_or_default()
    };
    let run_id_c = run_id.clone();
    let dispatch_id_c = dispatch_id.clone();
    let result = commit(move |_| {
        Ok(Cmd::DispatchUpdate {
            run_id: run_id_c, dispatch_id: dispatch_id_c,
            status: DispatchStatus::Stopped,
            summary: None,
            now,
        })
    }, run_id).await;
    match result {
        CommitResult::Ok(_) | CommitResult::Stale => {}
        CommitResult::Err(e) => return err_from_str(&e),
    }
    if !tile_id.is_empty() {
        if let Some(s) = crate::sessions().lock().unwrap().get(&tile_id).cloned() {
            crate::kill_session(&s);
        }
    }
    ok_resp(serde_json::json!({}))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::model::Limits;

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
            review: false,
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
}
