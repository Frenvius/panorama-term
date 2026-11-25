mod model;
mod autonomy;
mod candidates;
mod carry;
mod metrics;
mod plan;
mod usage;
mod routing;
mod wiring;
pub(crate) mod agy;
pub(crate) mod cli_env;
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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use serde_json::Value;
use crate::orch::model::*;
use crate::orch::reducer::{ApplyResult, Cmd, CoordinatorWake, IdleState, RunEntry, State, WaitRequest, WakeReason};
use crate::orch::store::{EventRecord, append_event, ensure_run_dir, load_into, remove_run_dir, run_events_path};
use crate::orch::profiles::{load_profiles, find_profile, resolve_env};
use crate::orch::worker::{BriefInput, WorkerLaunch, write_brief, spawn_worker, capture_worker_screen, maybe_nudge_tile, maybe_nudge_worker, is_tile_waiting};
use crate::ui::TileGroup;

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
static NOTIFIES: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Notify>>>> = OnceLock::new();
static COORDINATORS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static STARTING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static CANDIDATES: OnceLock<Mutex<candidates::CandidateStates>> = OnceLock::new();
static CANDIDATE_VERSION: AtomicU64 = AtomicU64::new(0);
static CANDIDATE_PERSISTED: Mutex<u64> = Mutex::new(0);

fn candidate_states() -> &'static Mutex<candidates::CandidateStates> {
    CANDIDATES.get_or_init(|| Mutex::new(candidates::CandidateStates::default()))
}

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

fn transition_notifications(before: Option<&(RunStatus, HashMap<String, TaskStatus>)>, after: &reducer::RunEntry, kind: &str, data: &Value) -> Vec<Value> {
    let mut events = Vec::new();
    let event = |name: &str, task: Option<&Task>, detail: Option<String>| serde_json::json!({
        "t": "orch.notify", "runId": after.run.id, "runTitle": after.run.title,
        "kind": name, "taskId": task.map(|t| &t.id), "taskTitle": task.map(|t| &t.title), "detail": detail
    });
    if kind == "gate-create" {
        let task = data["taskId"].as_str().and_then(|id| after.tasks.get(id));
        events.push(event("gate", task, data["question"].as_str().map(str::to_owned)));
    }
    if kind == "task-conflict" {
        let task = data["taskId"].as_str().and_then(|id| after.tasks.get(id));
        let detail = data["files"].as_array().map(|files| files.iter().filter_map(Value::as_str).take(5).collect::<Vec<_>>().join(", "));
        events.push(event("conflict", task, detail));
    }
    for task in after.tasks.values() {
        let old = before.and_then(|entry| entry.1.get(&task.id));
        if old == Some(&task.status) { continue; }
        if task.status == TaskStatus::Review {
            events.push(event("review", Some(task), None));
        } else if task.status == TaskStatus::Failed && task.attempts >= after.run.limits.max_attempts
            && data["summary"] != autonomy::STALL_REASON
        {
            events.push(event("exhausted", Some(task), None));
        }
    }
    if after.run.status == RunStatus::Completed && before.map(|entry| &entry.0) != Some(&RunStatus::Completed) {
        events.push(event("completed", None, None));
    }
    events
}

fn append_and_replay(
    guard: &mut State,
    run_id: &str,
    cmd: &Cmd,
    base: &std::path::Path,
) -> Result<(String, Value), CommitResult> {
    let event_seq = guard.runs.get(run_id).map(|e| e.event_seq + 1).unwrap_or(1);
    let now = unix_ms();
    match guard.apply(cmd) {
        ApplyResult::Ok(kind, data) => {
            if let Err(e) = ensure_run_dir(base, run_id) {
                return Err(CommitResult::Err(format!("io:{e}")));
            }
            let path = run_events_path(base, run_id);
            let record = EventRecord { seq: event_seq, at: now, kind: kind.clone(), data: data.clone() };
            if let Err(e) = append_event(&path, &record) {
                return Err(CommitResult::Err(format!("io:{e}")));
            }
            let before = guard.runs.get(run_id).map(|entry| (
                entry.run.status.clone(),
                entry.tasks.iter().map(|(id, task)| (id.clone(), task.status.clone())).collect(),
            ));
            if let Err(e) = guard.replay(&kind, &data) {
                eprintln!("[orch] replay after commit {kind}: {e}");
            } else if let Some(after) = guard.runs.get(run_id) {
                for event in transition_notifications(before.as_ref(), after, &kind, &data) {
                    crate::ui::broadcast_orch_notify(event);
                }
            }
            if let Some(entry) = guard.runs.get_mut(run_id) {
                entry.event_seq = event_seq;
            }
            Ok((kind, data))
        }
        ApplyResult::Stale(kind, data) => {
            let path = run_events_path(base, run_id);
            let record = EventRecord { seq: event_seq, at: now, kind: kind.clone(), data: data.clone() };
            if let Err(e) = append_event(&path, &record) {
                eprintln!("[orch] stale event append: {e}");
            } else if let Some(entry) = guard.runs.get_mut(run_id) {
                entry.event_seq = event_seq;
            }
            Err(CommitResult::Stale)
        }
        ApplyResult::Err(e) => Err(CommitResult::Err(format!("{}:{}", e.code(), e.message()))),
    }
}

const DEFAULT_QUESTION_ANSWER: &str = "No coordinator is available right now. Decide using the spec and your best judgment, note the decision and the reason in your report, and continue.";

fn idle_digest(entry: &RunEntry, report: &autonomy::IdleReport) -> String {
    let run_id = &entry.run.id;
    let refs = format!("orch report --run-id {run_id} --format markdown; orch run-show --run-id {run_id} --full");
    let mut tasks: Vec<_> = entry.tasks.values().collect();
    tasks.sort_by_key(|t| (t.id.len(), t.id.clone()));
    let budget = 3000usize.saturating_sub(refs.chars().count() + 1);
    let per_task = budget.saturating_sub(100) / tasks.len().max(1);
    let mut lines = Vec::new();
    for task in tasks {
        let dispatch = entry.latest_dispatch_for_task(&task.id);
        let verification = match dispatch {
            Some(d) if d.verifications.iter().any(|v| v.timed_out || v.exit_code != Some(0)) => "failed",
            Some(d) if !d.verifications.is_empty() => "passed",
            _ => "none",
        };
        let summary = dispatch.and_then(|d| entry.messages.iter().rev().find(|m| m.msg_type == "worker_done" && m.from == d.id && m.to == "coordinator"))
            .and_then(|m| m.body.as_deref()).and_then(|b| b.lines().nth(1)).unwrap_or("-");
        let fixed = format!("{} (): {:?}, {} attempts, verification , ", task.id, task.status, task.attempts);
        let available = per_task.saturating_sub(fixed.chars().count() + verification.len());
        let title: String = task.title.replace(['\r', '\n'], " ").chars().take(available / 3).collect();
        let summary: String = summary.chars().take(available.saturating_sub(title.chars().count())).collect();
        lines.push(format!("{} ({}): {:?}, {} attempts, verification {}, {}", task.id, title, task.status, task.attempts, verification, summary));
    }
    let groups = &report.tasks;
    for task in groups.failed.iter().chain(&groups.blocked).chain(&groups.waiting_intervention).chain(&groups.waiting_no_resume).chain(&groups.resume_due).chain(&groups.auto_action_failed).chain(&groups.open_gate) {
        let line = format!("{}: {}{}", task.detail.as_deref().unwrap_or("no reason recorded").lines().find(|line| !line.starts_with("Outcome:")).unwrap_or("no reason recorded"), task.id, if groups.resume_due.iter().any(|t| t.id == task.id) { " (resume due)" } else { "" });
        if lines.iter().map(|s| s.chars().count() + 1).sum::<usize>() + line.chars().count().min(160) + 1 <= budget {
            lines.push(line.chars().take(160).collect());
        }
    }
    for msg in entry.messages.iter().filter(|m| m.msg_type == "answer" && m.from == "brain" && m.body.as_deref() == Some(DEFAULT_QUESTION_ANSWER)) {
        if let Some(question) = entry.messages.iter().find(|q| Some(q.seq) == msg.reply_to && q.msg_type == "question") {
            let line = format!("Default answer to {}: {}", question.from, question.body.as_deref().unwrap_or("").lines().next().unwrap_or(""));
            if lines.iter().map(|s| s.chars().count() + 1).sum::<usize>() + line.chars().count().min(160) + 1 <= budget {
                lines.push(line.chars().take(160).collect());
            }
        }
    }
    lines.push(refs);
    lines.join("\n").chars().take(3000).collect()
}

fn sync_idle_locked(guard: &mut State, run_id: &str, base: &std::path::Path) -> bool {
    let mut answered = false;
    loop {
        let Some(entry) = guard.runs.get(run_id) else { return answered };
        if !entry.run.detached { break; }
        let question = entry.messages.iter().find(|q| {
            q.msg_type == "question" && !matches!(q.from.as_str(), "coordinator" | "user" | "brain")
                && !entry.messages.iter().any(|a| a.msg_type == "answer" && a.to == q.from && a.seq > q.seq)
        });
        let Some(question) = question else { break };
        let sender = question.from.clone();
        let cmd = Cmd::Send {
            run_id: run_id.into(), seq: entry.next_msg_seq(), from: "brain".into(), to: vec![sender.clone()],
            msg_type: "answer".into(), task_id: entry.dispatches.get(&sender).map(|d| d.task_id.clone()),
            dispatch_id: Some(sender), reply_to: Some(question.seq), body: Some(DEFAULT_QUESTION_ANSWER.into()),
            outcome: None, files_modified: vec![], summary: None, now: unix_ms(),
        };
        if append_and_replay(guard, run_id, &cmd, base).is_err() { break; }
        answered = true;
    }
    let Some(entry) = guard.runs.get(run_id) else { return false };
    let now = unix_ms();
    let report = autonomy::idle_reason(entry, now as i64);
    let desired = match &report {
        None => IdleState::Active,
        Some(r) if r.reason == "finished" => IdleState::Finished,
        Some(_) => IdleState::Idle,
    };
    if desired == entry.idle_state {
        let unchanged = report.as_ref().map(|r| {
            let msg_type = if desired == IdleState::Finished { "run_finished" } else { "run_idle" };
            let body = idle_digest(entry, r);
            entry.idle_fingerprint.as_deref() == Some(&reducer::idle_fingerprint(msg_type, &body))
        }).unwrap_or(true);
        if unchanged { return answered; }
    }
    let cmd = match desired {
        IdleState::Active => Cmd::RunActive { run_id: run_id.to_string(), now },
        IdleState::Idle => Cmd::RunIdle {
            run_id: run_id.to_string(),
            msg_seq: entry.next_msg_seq(),
            msg_type: "run_idle".into(),
            body: idle_digest(entry, report.as_ref().unwrap()),
            now,
        },
        IdleState::Finished => Cmd::RunIdle {
            run_id: run_id.to_string(),
            msg_seq: entry.next_msg_seq(),
            msg_type: "run_finished".into(),
            body: idle_digest(entry, report.as_ref().unwrap()),
            now,
        },
    };
    append_and_replay(guard, run_id, &cmd, base).is_ok() || answered
}

fn wake_tile_for_messages(entry: &RunEntry, previous_messages: usize, now: i64) -> Option<String> {
    entry.messages.iter().skip(previous_messages)
        .any(|m| m.to == "coordinator" && needs_decision(m, entry, now))
        .then(|| entry.run.coordinator_tile.clone()).flatten()
}

fn commit_sync_raw<F>(make_cmd: F, run_id: &str, base: &std::path::Path) -> CommitResult
where
    F: FnOnce(&State) -> Result<Cmd, String>,
{
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let cmd = match make_cmd(&guard) {
        Ok(c) => c,
        Err(e) => return CommitResult::Err(e),
    };
    let previous_messages = guard.runs.get(run_id).map_or(0, |e| e.messages.len());
    let (kind, data) = match append_and_replay(&mut guard, run_id, &cmd, base) {
        Ok(pair) => pair,
        Err(result) => return result,
    };
    let now = unix_ms() as i64;
    let wake = guard.runs.get(run_id).and_then(|entry| {
        wake_tile_for_messages(entry, previous_messages, now)
            .or_else(|| (kind == "run-update" && data["detached"] == false)
                .then(|| entry.run.coordinator_tile.clone()).flatten())
    });
    drop(guard);
    if let Some(tile) = wake {
        recheck_coordinator(run_id, &tile);
    }
    CommitResult::Ok(data)
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
    let previous_messages = guard.runs.get(run_id).map_or(0, |e| e.messages.len());
    let (kind, data) = match append_and_replay(&mut guard, run_id, &cmd, base) {
        Ok(pair) => pair,
        Err(result) => return result,
    };
    sync_idle_locked(&mut guard, run_id, base);
    let now = unix_ms() as i64;
    let wake = guard.runs.get(run_id).and_then(|entry| {
        wake_tile_for_messages(entry, previous_messages, now)
            .or_else(|| (kind == "run-update" && data["detached"] == false)
                .then(|| entry.run.coordinator_tile.clone()).flatten())
    });
    drop(guard);
    if let Some(tile) = wake {
        recheck_coordinator(run_id, &tile);
    }
    CommitResult::Ok(data)
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

async fn commit_deferred_idle<F>(make_cmd: F, run_id: String) -> CommitResult
where
    F: FnOnce(&State) -> Result<Cmd, String> + Send + 'static,
{
    let Some(base) = crate::panorama_dir() else {
        return CommitResult::Err("io:no panorama dir".into());
    };
    let notify_id = run_id.clone();
    let result = tokio::task::spawn_blocking(move || commit_sync_raw(make_cmd, &run_id, &base))
        .await
        .unwrap_or_else(|_| CommitResult::Err("io:spawn failed".into()));
    if !matches!(result, CommitResult::Err(_)) {
        run_notify(&notify_id).notify_waiters();
        crate::ui::broadcast_orch_changed(&notify_id);
    }
    result
}

async fn sync_idle(run_id: &str) {
    let Some(base) = crate::panorama_dir() else { return };
    let run_id_c = run_id.to_string();
    let changed = tokio::task::spawn_blocking(move || {
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let previous_messages = guard.runs.get(&run_id_c).map_or(0, |e| e.messages.len());
        let changed = sync_idle_locked(&mut guard, &run_id_c, &base);
        let wake = changed.then(|| {
            let now = unix_ms() as i64;
            guard.runs.get(&run_id_c)
                .and_then(|entry| wake_tile_for_messages(entry, previous_messages, now))
        }).flatten();
        drop(guard);
        if let Some(tile) = wake {
            recheck_coordinator(&run_id_c, &tile);
        }
        changed
    }).await.unwrap_or(false);
    if changed {
        run_notify(run_id).notify_waiters();
        crate::ui::broadcast_orch_changed(run_id);
    }
}

struct CheckOptions {
    recipient: String,
    types: Option<Vec<String>>,
    peek: bool,
    decisions: bool,
}

#[derive(Default)]
struct RoutineDigest {
    count: usize,
    by_type: std::collections::BTreeMap<String, usize>,
    tasks: std::collections::BTreeMap<String, Value>,
}

impl RoutineDigest {
    fn add(&mut self, msg: &Message, entry: &RunEntry) {
        self.count += 1;
        *self.by_type.entry(msg.msg_type.clone()).or_default() += 1;
        let task = msg.task_id.as_ref().or_else(|| {
            let dispatch = msg.dispatch_id.as_deref().unwrap_or(&msg.from);
            entry.dispatches.get(dispatch).map(|d| &d.task_id)
        }).and_then(|id| entry.tasks.get(id));
        if let Some(task) = task {
            self.tasks.insert(task.id.clone(), serde_json::json!({
                "id": task.id, "title": task.title, "status": task.status,
            }));
        }
    }

    fn response(&self, messages: Vec<Value>, decisions: bool) -> String {
        let mut result = serde_json::json!({"messages": messages});
        if decisions {
            result["routine"] = serde_json::json!({
                "count": self.count,
                "byType": self.by_type,
                "tasks": self.tasks.values().collect::<Vec<_>>(),
            });
        }
        ok_resp(result)
    }
}

fn collect_ack_sync(
    run_id: &str,
    options: &CheckOptions,
    base: &std::path::Path,
    waiting: bool,
) -> Result<(Vec<Value>, RoutineDigest), String> {
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some(entry) = guard.runs.get(run_id) else {
        return Err("not_found:run not found".into());
    };

    let now = unix_ms() as i64;
    let mut matched_seqs = Vec::new();
    let mut msgs = Vec::new();
    let mut routine_msgs = Vec::new();
    for msg in entry.messages.iter().filter(|m| !m.acked && m.to == options.recipient) {
        if options.decisions && !needs_decision(msg, entry, now) {
            if !options.peek {
                matched_seqs.push(msg.seq);
                routine_msgs.push(msg);
            }
        } else if options.types.as_ref().is_none_or(|types| types.contains(&msg.msg_type)) {
            matched_seqs.push(msg.seq);
            msgs.push(serde_json::to_value(msg).map_err(|e| format!("io:{e}"))?);
        }
    }

    if waiting && msgs.is_empty() {
        return Ok((msgs, RoutineDigest::default()));
    }
    let mut routine = RoutineDigest::default();
    for msg in routine_msgs {
        routine.add(msg, entry);
    }
    if options.peek || matched_seqs.is_empty() {
        return Ok((msgs, routine));
    }

    let event_seq = entry.event_seq + 1;
    let ack_cmd = Cmd::Ack { run_id: run_id.to_string(), seqs: matched_seqs };

    let (kind, data) = match guard.apply(&ack_cmd) {
        ApplyResult::Ok(k, d) => (k, d),
        _ => return Ok((msgs, RoutineDigest::default())),
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

    Ok((msgs, routine))
}

pub fn init() {
    let Some(base) = crate::panorama_dir() else { return };

    let loaded = load_coordinators(&base);
    *coordinators().lock().unwrap_or_else(|e| e.into_inner()) = loaded;
    restore_candidates(&base, candidate_states());

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
        let run_ids: Vec<_> = guard.runs.keys().cloned().collect();
        for run_id in run_ids { sync_idle_locked(&mut guard, &run_id, &base); }
        for entry in guard.runs.values().filter(|entry| !entry.run.status.is_terminal()) {
            restart_verifications(entry).for_each(drop);
        }
    }
    tokio::runtime::Handle::current().spawn(supervision_loop());
}

pub async fn handle(command: &str, body: &[u8]) -> String {
    match command {
        "run-create" => handle_run_create(body).await,
        "plan-apply" => plan::handle(body).await,
        "run-update" => handle_run_update(body).await,
        "run-list" => handle_run_list(),
        "run-show" => handle_run_show(body),
        "run-set-status" => handle_run_set_status(body).await,
        "run-delete" => handle_run_delete(body).await,
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
        "worker-transcript" => handle_worker_transcript(body),
        "worker-send" => handle_worker_send(body),
        "task-cancel" => handle_task_cancel(body).await,
        "worker-stop" => handle_worker_stop(body).await,
        "worker-release" => handle_worker_release(body).await,
        "integrate" => handle_integrate(body).await,
        "worktree-clean" => handle_worktree_clean(body).await,
        "verify" => handle_verify(body).await,
        "retry" => handle_retry(body).await,
        "conflict-resolve" => handle_conflict_resolve(body).await,
        "report" => handle_report(body).await,
        "candidate-list" => handle_candidate_list(),
        "candidate-clear" => handle_candidate_clear(body),
        "usage-report" => handle_usage_report(body),
        "metrics" => handle_metrics().await,
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
        "ai-progress" => handle_ai_progress(body),
        _ => err_resp("bad_request", "unknown command"),
    }
}

pub(crate) fn recheck_coordinator(run_id: &str, tile_id: &str) {
    let now = unix_ms() as i64;
    let (reason, count, detached) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(run_id).filter(|e| !e.run.status.is_terminal()
            && e.run.coordinator_tile.as_deref() == Some(tile_id)) else { return };
        let mut seen = HashSet::new();
        let mut types = Vec::new();
        let mut count = 0;
        for message in coordinator_unacked(entry, now) {
            count += 1;
            if seen.insert(message.msg_type.as_str()) {
                types.push(message.msg_type.clone());
            }
        }
        (WakeReason::Messages(types), count, entry.run.detached)
    };
    if count != 0 {
        maybe_nudge_tile(tile_id, run_id, reason, count, detached);
    }
}

fn record_coordinator_wake(run_id: &str, reason: WakeReason, message_count: usize, detached: bool) {
    let wake = CoordinatorWake { reason, message_count, detached, at: unix_ms() };
    let Some(base) = crate::panorama_dir() else { return };
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let cmd = Cmd::RunCoordinatorWake { run_id: run_id.to_string(), wake };
    if let Err(CommitResult::Err(e)) = append_and_replay(&mut guard, run_id, &cmd, &base) {
        eprintln!("[orch] record coordinator wake: {e}");
    }
}

fn is_current_idle_msg(msg: &Message, entry: &RunEntry, now: i64, expected_reason: &str) -> bool {
    let is_latest = entry.messages.iter().rev()
        .find(|m| matches!(m.msg_type.as_str(), "run_idle" | "run_finished"))
        .is_some_and(|last| last.seq == msg.seq);
    is_latest && autonomy::idle_reason(entry, now)
        .is_some_and(|report| report.reason == expected_reason)
}

fn needs_decision(msg: &Message, entry: &RunEntry, now: i64) -> bool {
    if !entry.run.detached {
        return true;
    }
    match msg.msg_type.as_str() {
        "integration_conflict" if msg.from == "brain" => msg.task_id.as_ref().and_then(|id| entry.tasks.get(id))
            .is_none_or(|task| !entry.run.autonomy.integration
                || (entry.integration_conflicts.contains(&task.id) && !autonomy::can_resolve_conflict(entry, task))),
        "worker_stalled" if msg.from == "brain" => msg.dispatch_id.as_ref().and_then(|id| entry.dispatches.get(id))
            .is_none_or(|dispatch| {
                if !entry.run.autonomy.operational {
                    return true;
                }
                let continued = autonomy::stall_continuation(entry, &dispatch.id)
                    .is_some_and(|c| c.seq > msg.seq);
                let stopped = dispatch.status == DispatchStatus::Stopped
                    && dispatch.summary.as_deref() == Some(autonomy::STALL_REASON);
                !continued && !stopped && !entry.tasks.get(&dispatch.task_id).is_some_and(|task| {
                    autonomy::can_handle_stall(entry, task, dispatch)
                        && dispatch.tile_id.as_deref().is_some_and(|tile| crate::lookup_live(tile).is_some())
                })
            }),
        "integration_conflict" | "worker_stalled" | "setup_failed" | "gate" => true,
        "note" => msg.from == "user",
        "run_idle" => is_current_idle_msg(msg, entry, now, "idle"),
        "run_finished" => is_current_idle_msg(msg, entry, now, "finished"),
        _ => false,
    }
}

fn coordinator_unacked(entry: &RunEntry, now: i64) -> impl Iterator<Item = &Message> {
    entry.messages.iter()
        .filter(move |m| !m.acked && m.to == "coordinator" && needs_decision(m, entry, now))
}

pub fn on_agent_idle(tile_id: &str) {
    let cli = crate::panorama_cli_path();
    let now = unix_ms() as i64;
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    for (run_id, entry) in &guard.runs {
        if entry.run.status.is_terminal() { continue; }
        if let Some(ref coord_tile) = entry.run.coordinator_tile {
            if coord_tile == tile_id {
                if coordinator_unacked(entry, now).next().is_some() {
                    let run_id = run_id.clone();
                    drop(guard);
                    recheck_coordinator(&run_id, tile_id);
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
                    });
                    return;
                }
            }
        }
    }
}

fn parse_autonomy_field(v: &Value) -> Result<Option<Autonomy>, String> {
    let Some(raw) = v.get("autonomy").filter(|a| !a.is_null()) else {
        return Ok(None);
    };
    if raw.is_object() {
        return serde_json::from_value(raw.clone())
            .map(Some)
            .map_err(|_| err_resp("bad_request", "autonomy object is invalid"));
    }
    let Some(list) = raw.as_str() else {
        return Err(err_resp("bad_request", "autonomy must be a comma separated string"));
    };
    let mut autonomy = Autonomy::default();
    for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        match name {
            "operational" => autonomy.operational = true,
            "execution" => autonomy.execution = true,
            "integration" => autonomy.integration = true,
            other => return Err(err_resp("bad_request", &format!(
                "unknown autonomy '{other}'; use operational, execution, integration"
            ))),
        }
    }
    Ok(Some(autonomy))
}

fn run_extra_fields(autonomy: Option<Autonomy>, max_operational_attempts: Option<u64>, detached: Option<bool>) -> Value {
    let mut fields = serde_json::Map::new();
    if let Some(a) = autonomy {
        fields.insert("autonomy".into(), serde_json::to_value(a).unwrap_or_default());
    }
    if let Some(n) = max_operational_attempts {
        fields.insert("maxOperationalAttempts".into(), n.into());
    }
    if let Some(detached) = detached {
        fields.insert("detached".into(), detached.into());
    }
    Value::Object(fields)
}

fn parse_run_bool(v: &Value, field: &str) -> Result<Option<bool>, String> {
    match &v[field] {
        Value::Null => Ok(None),
        Value::Bool(enabled) => Ok(Some(*enabled)),
        _ => Err(err_resp("bad_request", &format!("{field} must be a boolean"))),
    }
}

fn parse_worker_summary_cap(v: &Value) -> Result<Option<u32>, String> {
    let cap = &v["workerSummaryCap"];
    if cap.is_null() { return Ok(None); }
    cap.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|&n| n > 0).map(Some)
        .ok_or_else(|| err_resp("bad_request", "workerSummaryCap must be a positive 32-bit integer"))
}

fn run_create_fields(v: &Value) -> Result<Value, String> {
    let detached = parse_run_bool(v, "detached")?;
    let autonomy = parse_autonomy_field(v)?.or_else(|| {
        (detached == Some(true)).then_some(Autonomy {
            operational: true, execution: true, integration: true,
        })
    });
    let mut fields = run_extra_fields(autonomy, None, detached);
    if let Some(enabled) = parse_run_bool(v, "correctiveRetry")? {
        fields["correctiveRetry"] = enabled.into();
    }
    if let Some(cap) = parse_worker_summary_cap(v)? {
        fields["workerSummaryCap"] = cap.into();
    }
    Ok(fields)
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
    let moa = get_u64("maxOperationalAttempts").unwrap_or(6);
    if mw < 1 { return Err(err_resp("bad_request", "maxWorkers must be >= 1")); }
    if ma < 1 { return Err(err_resp("bad_request", "maxAttempts must be >= 1")); }
    if dt < 1 { return Err(err_resp("bad_request", "dispatchTimeoutMin must be >= 1")); }
    Ok(Limits {
        max_workers: mw as u32,
        max_attempts: ma as u32,
        dispatch_timeout_min: dt as u32,
        verify_timeout_min: vt as u32,
        stall_quiet_min: sq as u32,
        max_operational_attempts: moa as u32,
    })
}

async fn run_create_cmd(v: &Value, id: String) -> Result<Cmd, String> {
    let repo = v["repo"].as_str().unwrap_or("").to_string();
    if repo.is_empty() { return Err(err_resp("bad_request", "repo required")); }
    if !std::path::Path::new(&repo).is_dir() {
        return Err(err_resp("bad_request", "repo is not an existing directory"));
    }
    let limits = parse_limits(v)?;
    let fields = run_create_fields(v)?;
    let title = v["title"].as_str().unwrap_or("untitled").to_string();
    let base_ref = v["baseRef"].as_str().map(str::to_string);
    let setup = v["setup"].as_str().map(str::to_string);
    let coordinator_tile = v["coordinatorTile"].as_str().map(str::to_string);

    let rev = base_ref.clone().unwrap_or_else(|| "HEAD".into());
    let repo_c = repo.clone();
    let base_sha = tokio::task::spawn_blocking(move || {
        crate::orch::git::rev_parse(&repo_c, &rev).ok()
    }).await.unwrap_or(None);

    let now = unix_ms();
    let cmd = Cmd::RunCreate { id, title, repo, base_ref, base_sha, setup, coordinator_tile, limits, now };
    Ok(cmd.with_fields(fields))
}

async fn created_run(id: &str) -> Value {
    let (run, tile) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.runs.get(id);
        let run = entry.map(|e| serde_json::to_value(&e.run).unwrap_or_default()).unwrap_or_default();
        let tile = entry.and_then(|e| e.run.coordinator_tile.clone());
        (run, tile)
    };
    if let Some(tile) = tile {
        let cost_usd = crate::tile_cost_usd(&tile);
        let run_id = id.to_owned();
        let now = unix_ms();
        let _ = commit(move |_| {
            Ok(Cmd::RunRecordCoordinatorBaseline { run_id, cost_usd, now })
        }, id.to_owned()).await;
    }
    run
}

async fn handle_run_create(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let id = gen_run_id();
    let cmd = match run_create_cmd(&v, id.clone()).await { Ok(cmd) => cmd, Err(e) => return e };
    match commit(move |_| Ok(cmd), id.clone()).await {
        CommitResult::Ok(_) => ok_resp(serde_json::json!({"run": created_run(&id).await})),
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
        let latest_activity = e.dispatches.values().flat_map(|d| [Some(d.started_at), d.ended_at, d.last_heartbeat])
            .flatten().chain(e.messages.iter().map(|m| m.at)).max().unwrap_or(0).max(e.run.updated_at);
        let mut row = serde_json::json!({"run": e.run, "taskCounts": counts, "openGates": open_gates,
            "runningDispatches": e.running_dispatch_count(), "latestActivity": latest_activity,
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
    let full = v["full"].as_bool().unwrap_or(false);

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
            let mut val = serde_json::to_value(d).unwrap_or_default();
            if full {
                if let Some(late) = entry.late_reports.get(&d.id) {
                    val["lateReports"] = serde_json::json!(late);
                }
            } else {
                val.as_object_mut().map(|obj| obj.remove("report"));
            }
            DispatchRow {
                val,
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
                "trigger": d.get("trigger"),
                "failureClass": d.get("failureClass"),
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
                "qualityAttempts": t["qualityAttempts"],
                "operationalAttempts": t["operationalAttempts"],
                "wait": t.get("wait"),
                "ready": t["ready"]
            })
        }).collect();
        let gates_brief: Vec<Value> = gates.iter()
            .filter(|g| g["resolution"].is_null())
            .map(|g| serde_json::json!({"id": g["id"], "taskId": g["taskId"], "question": g["question"]}))
            .collect();
        return ok_resp(serde_json::json!({
            "run": {"id": run_val["id"], "title": run_val["title"], "status": run_val["status"], "detached": run_val["detached"], "correctiveRetry": run_val["correctiveRetry"], "workerSummaryCap": run_val["workerSummaryCap"]},
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

pub(crate) fn on_native_session_bound(tile_id: &str, agent: &str, id: &str, cwd: Option<&str>) {
    if !matches!(agent, "claude" | "codex") || !worker::valid_session_id(id) {
        return;
    }
    let Some(base) = crate::panorama_dir() else { return };
    let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let Some((run_id, dispatch_id)) = guard.runs.iter().find_map(|(run_id, entry)| {
        entry.dispatches.values().find(|d| {
            d.tile_id.as_deref() == Some(tile_id)
                && d.native_session.is_none()
                && d.accepts_reports()
                && d.worktree.as_deref().zip(cwd).is_none_or(|(worktree, cwd)|
                    crate::path_key(&crate::native_path(worktree)) == crate::path_key(&crate::native_path(cwd)))
        }).map(|d| (run_id.clone(), d.id.clone()))
    }) else { return };
    let cmd = Cmd::DispatchBindSession {
        run_id: run_id.clone(), dispatch_id,
        session: model::NativeSession { agent: agent.into(), id: id.into() },
        now: unix_ms(),
    };
    if let Err(CommitResult::Err(e)) = append_and_replay(&mut guard, &run_id, &cmd, &base) {
        eprintln!("[orch] session binding not recorded: {e}");
    }
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

async fn handle_run_delete(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let Some(base) = crate::panorama_dir() else { return err_resp("io", "no panorama dir"); };

    let (live, worker_tiles) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(&run_id) else { return err_resp("not_found", "run not found"); };
        let live = matches!(entry.run.status, RunStatus::Active | RunStatus::Paused);
        let tiles: Vec<(String, String)> = entry.dispatches.values().filter_map(|d| d.tile_id.clone().map(|t| (d.id.clone(), t))).collect();
        (live, tiles)
    };

    if live {
        let run_id_c = run_id.clone();
        let now = unix_ms();
        let result = commit(move |_| {
            Ok(Cmd::RunSetStatus { run_id: run_id_c, status: RunStatus::Cancelled, now })
        }, run_id.clone()).await;
        if let CommitResult::Err(e) = result { return err_from_str(&e); }
    }

    let mut released = false;
    for (dispatch_id, tile) in &worker_tiles {
        if let Err(e) = save_worker_transcript(&run_id, dispatch_id, tile) { eprintln!("[orch] transcript run {run_id} dispatch {dispatch_id}: {e}"); }
        if let Some(session) = crate::lookup_live(tile) {
            crate::kill_session(&session);
            released = true;
        }
    }
    if released {
        tokio::time::sleep(Duration::from_millis(SESSION_RELEASE_SETTLE_MS)).await;
    }

    let clean: Value = serde_json::from_str(&handle_worktree_clean(body).await).unwrap_or_default();
    let kept = clean.get("kept").cloned().unwrap_or_else(|| serde_json::json!([]));

    {
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = remove_run_dir(&base, &run_id) {
            return err_resp("io", &e);
        }
        guard.runs.remove(&run_id);
    }
    notifies().lock().unwrap_or_else(|e| e.into_inner()).remove(&run_id);
    crate::ui::broadcast_orch_changed(&run_id);
    ok_resp(serde_json::json!({ "kept": kept }))
}

async fn handle_run_update(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let title = v["title"].as_str().unwrap_or("").to_string();
    let autonomy = match parse_autonomy_field(&v) { Ok(a) => a, Err(e) => return e };
    let detached = match parse_run_bool(&v, "detached") { Ok(d) => d, Err(e) => return e };
    let max_operational = v.get("maxOperationalAttempts").filter(|n| !n.is_null()).and_then(|n| n.as_u64());
    if v.get("maxOperationalAttempts").map(|n| !n.is_null()).unwrap_or(false) && max_operational.map(|n| n < 1).unwrap_or(true) {
        return err_resp("bad_request", "maxOperationalAttempts must be an integer >= 1");
    }
    let corrective = match parse_run_bool(&v, "correctiveRetry") { Ok(c) => c, Err(e) => return e };
    let summary_cap = match parse_worker_summary_cap(&v) { Ok(cap) => cap, Err(e) => return e };
    if title.is_empty() && autonomy.is_none() && max_operational.is_none() && detached.is_none() && corrective.is_none() && summary_cap.is_none() {
        return err_resp("bad_request", "title, autonomy, maxOperationalAttempts, detached, correctiveRetry or workerSummaryCap required");
    }
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let result = commit(move |state| {
        let title = if title.is_empty() {
            state.runs.get(&run_id_c).map(|e| e.run.title.clone()).unwrap_or_default()
        } else {
            title
        };
        let cmd = Cmd::RunUpdate { run_id: run_id_c, title, now };
        let mut fields = run_extra_fields(autonomy, max_operational, detached);
        if let Some(enabled) = corrective {
            fields["correctiveRetry"] = enabled.into();
        }
        if let Some(cap) = summary_cap {
            fields["workerSummaryCap"] = cap.into();
        }
        Ok(cmd.with_fields(fields))
    }, run_id).await;
    match result {
        CommitResult::Ok(_) => ok_resp(serde_json::json!({})),
        CommitResult::Stale => err_resp("stale", "run update stale"),
        CommitResult::Err(e) => err_from_str(&e),
    }
}

struct CandidateSnapshot {
    version: u64,
    states: candidates::CandidateStates,
}

fn snapshot_candidates(states: &candidates::CandidateStates) -> CandidateSnapshot {
    CandidateSnapshot {
        version: CANDIDATE_VERSION.fetch_add(1, Ordering::SeqCst) + 1,
        states: states.clone(),
    }
}

fn restore_candidates(base: &std::path::Path, target: &Mutex<candidates::CandidateStates>) {
    *target.lock().unwrap_or_else(|e| e.into_inner()) = candidates::load(base);
}

fn persist_candidates(snapshot: &CandidateSnapshot) -> Result<(), String> {
    let Some(base) = crate::panorama_dir() else {
        return Err(err_resp("io", "no panorama dir"));
    };
    persist_candidates_at(&base, snapshot, &CANDIDATE_PERSISTED)
}

fn persist_candidates_at(base: &std::path::Path, snapshot: &CandidateSnapshot, persisted: &Mutex<u64>) -> Result<(), String> {
    let mut last = persisted.lock().unwrap_or_else(|e| e.into_inner());
    if snapshot.version <= *last {
        return Ok(());
    }
    candidates::save(base, &snapshot.states).map_err(|e| err_resp("io", &e))?;
    *last = snapshot.version;
    Ok(())
}

fn handle_candidate_list() -> String {
    let now = unix_ms() as i64;
    let guard = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
    ok_resp(serde_json::json!({"candidates": guard.entries(now)}))
}

fn handle_candidate_clear(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let agent = v["agent"].as_str().unwrap_or("").to_string();
    if agent.is_empty() { return err_resp("bad_request", "agent required"); }
    let filter = candidates::CandidateKey {
        agent,
        model: v["model"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
        account: v["account"].as_str().filter(|s| !s.is_empty()).map(str::to_string),
    };
    let (removed, snapshot) = {
        let mut guard = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
        let removed = guard.clear(&filter);
        (removed, (removed > 0).then(|| snapshot_candidates(&guard)))
    };
    if let Some(snapshot) = snapshot {
        if let Err(e) = persist_candidates(&snapshot) { return e; }
    }
    ok_resp(serde_json::json!({"cleared": removed}))
}

fn handle_usage_report(body: &[u8]) -> String {
    let mut v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    if let Some(raw) = v.get("windows").and_then(|w| w.as_str()).map(str::to_string) {
        match serde_json::from_str::<Value>(&raw) {
            Ok(parsed) => v["windows"] = parsed,
            Err(_) => return err_resp("bad_request", "windows must be a JSON array"),
        }
    }
    let report: candidates::UsageReport = match serde_json::from_value(v) {
        Ok(r) => r,
        Err(e) => return err_resp("bad_request", &format!("invalid usage report: {e}")),
    };
    if report.agent.is_empty() || report.account_id.is_empty() {
        return err_resp("bad_request", "agent and accountId required");
    }
    let now = unix_ms() as i64;
    let snapshot = {
        let mut guard = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
        let before = guard.entries(now);
        guard.apply_usage_report(&report, now);
        (guard.entries(now) != before).then(|| snapshot_candidates(&guard))
    };
    if let Some(snapshot) = snapshot {
        if let Err(e) = persist_candidates(&snapshot) { return e; }
    }
    ok_resp(serde_json::json!({}))
}

async fn handle_metrics() -> String {
    let snapshot = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        metrics::snapshot(guard.runs.values())
    };
    match tokio::task::spawn_blocking(move || metrics::compute(snapshot)).await {
        Ok(metrics) => ok_resp(metrics),
        Err(e) => err_resp("io", &e.to_string()),
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

fn task_create_cmd(
    v: &Value, run_id: String, deps: Vec<String>,
) -> Result<impl FnOnce(String) -> Cmd, String> {
    let title = v["title"].as_str().unwrap_or("untitled").to_string();
    let spec = v["spec"].as_str().unwrap_or("").to_string();
    let acceptance = v["acceptance"].as_str().map(str::to_string);
    let profile = v["profile"].as_str().map(str::to_string);
    let review = v["review"].as_bool().unwrap_or(false);
    let owner: Option<String> = v["owner"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let now = unix_ms();
    let pinned = profile.is_some();

    let mut escalation = strict_str_arr(v, "escalation")?;
    let verify = strict_str_arr(v, "verify")?;

    let role: Option<model::Role> = match v.get("role").filter(|r| !r.is_null()) {
        None => None,
        Some(r) => match serde_json::from_value::<model::Role>(r.clone()) {
            Ok(role) => Some(role),
            Err(_) => return Err(err_resp("bad_role", &format!(
                "role must be one of: {}; got: {}",
                model::Role::all().join(", "),
                r.as_str().unwrap_or("(invalid)")
            ))),
        },
    };

    if escalation.is_empty() && profile.is_none() {
        let Some(base) = crate::panorama_dir() else {
            return Err(err_resp("io", "no panorama dir"));
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
            return Err(err_resp("empty_role_chain", &format!(
                "role chain for '{effective_role}' is empty; set it in Settings > Orchestration or pass --escalation"
            )));
        }
        escalation = chain.clone();
    }

    Ok(move |id| {
        let cmd = Cmd::TaskCreate {
            run_id, id, title, spec, acceptance, deps, profile, escalation, verify, role,
            review, owner, now,
        };
        cmd.with_fields(serde_json::json!({"routing": 1, "pinned": pinned}))
    })
}

async fn handle_task_create(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }
    let deps = match strict_str_arr(&v, "deps") { Ok(a) => a, Err(e) => return e };
    let make_cmd = match task_create_cmd(&v, run_id.clone(), deps) {
        Ok(make_cmd) => make_cmd,
        Err(e) => return e,
    };
    let by = v["by"].as_str().unwrap_or("coordinator");
    let title = v["title"].as_str().unwrap_or("untitled");
    let run_id_c = run_id.clone();
    let result = commit(move |state| {
        let entry = state.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        Ok(make_cmd(entry.next_task_id()))
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
    let owner = match v.get("owner").filter(|v| !v.is_null()) {
        None => None,
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        _ => return err_resp("bad_request", "owner must be user or coordinator"),
    };
    let reason = v["reason"].as_str().map(str::to_string);
    let now = unix_ms();

    let prev_status = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.tasks.get(&task_id))
            .map(|t| t.status.clone())
    };

    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let actor = by.clone();
    let commit_result = commit(move |_| {
        Ok(Cmd::TaskUpdate {
            run_id: run_id_c, task_id: task_id_c, status, result: result_val,
            spec, acceptance, verify, profile, escalation, review: review_field, owner, by: actor, reason, now,
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
            attempt, retry_of, profile, model, tile_id, owner: None, merge_branch: None, now,
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
    let dispatch_id_for_hook = if msg_type == "worker_done" && !matches!(from.as_str(), "coordinator" | "user" | "brain") {
        Some(from.clone())
    } else {
        dispatch_id.clone()
    };
    let reply_to = v["replyTo"].as_u64().map(|n| n as u32);
    let worker_report = || v["report"].as_str().or_else(|| v["body"].as_str()).map(str::to_string);
    let body_str = if msg_type == "worker_done" { worker_report() } else { v["body"].as_str().map(str::to_string) };
    let outcome = v["outcome"].as_str().map(str::to_string);
    let summary = v["summary"].as_str().map(str::to_string);
    if msg_type == "worker_done" {
        if let Some(did) = dispatch_id_for_hook.as_deref() {
            commit_worker_leftovers(&run_id, did).await;
        }
    }
    let failure = match (msg_type.as_str(), outcome.as_deref(), dispatch_id_for_hook.as_deref()) {
        ("worker_done", Some("failed"), Some(did)) => {
            classify_dispatch(&run_id, did, "worker_failed_report", &dispatch_screen_tail(&run_id, did))
        }
        _ => None,
    };
    let failure_c = failure.clone();
    let now = unix_ms();
    let run_id_c = run_id.clone();
    let msg_type_c = msg_type.clone();
    let from_c = from.clone();
    let files_modified_c = files_modified.clone();

    let routed_failure = failure.as_ref().is_some_and(|f| f.routing == 1);
    let successful_report = msg_type == "worker_done" && outcome.as_deref() == Some("succeeded");
    let make_send = move |state: &State| {
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
        Ok(with_failure_class(Cmd::Send {
            run_id: run_id_c, seq, from: from_c, to,
            msg_type: msg_type_c, task_id, dispatch_id, reply_to,
            body: body_str, outcome, files_modified: files_modified_c, summary, now,
        }, failure_c.as_ref()))
    };
    let result = if routed_failure || successful_report {
        commit_deferred_idle(make_send, run_id.clone()).await
    } else {
        commit(make_send, run_id.clone()).await
    };

    match result {
        CommitResult::Ok(_) => {
            if let (Some(f), Some(did)) = (&failure, dispatch_id_for_hook.as_deref()) {
                send_follow_up(after_recorded_failure(&run_id, did, f, false).await).await;
            }
            if msg_type == "worker_done" && v["outcome"].as_str() == Some("succeeded") {
                if let Some(did) = dispatch_id_for_hook.as_deref() {
                    clear_candidate_on_success(&run_id, did);
                }
            }
            if msg_type == "worker_done" {
                if let Some(did) = dispatch_id_for_hook {
                    let run_id_wd = run_id.clone();
                    let verification = successful_report.then(|| VerificationFlight::for_report(&run_id, &did)).flatten();
                    tokio::spawn(async move {
                        worker_done_side_effects(run_id_wd, did, verification).await;
                    });
                }
            }
            if successful_report {
                sync_idle(&run_id).await;
            }
            ok_resp(serde_json::json!({}))
        }
        CommitResult::Stale => {
            if msg_type == "worker_done" {
                if let Some(did) = dispatch_id_for_hook {
                    let run_id_lr = run_id.clone();
                    let outcome_lr = v["outcome"].as_str().unwrap_or("").to_string();
                    let summary_lr = v["summary"].as_str().unwrap_or("").to_string();
                    let report_lr = worker_report();
                    let files_lr = files_modified.clone();
                    tokio::spawn(async move {
                        let now_lr = unix_ms();
                        let body_lr = serde_json::json!({
                            "dispatchId": did,
                            "outcome": outcome_lr,
                            "summary": summary_lr,
                            "report": report_lr,
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

async fn worker_done_side_effects(run_id: String, dispatch_id: String, verification: Option<VerificationFlight>) {
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
        let wt = d.worktree.clone();
        let task_id = d.task_id.clone();
        let tile_id = d.tile_id.clone();
        let dispatch_profile = d.profile.clone();
        let title = entry.tasks.get(&task_id).map(|t| t.title.clone()).unwrap_or_default();
        (wt, entry.run.base_sha.clone(), format!("orch({task_id}): {title}"), task_id, tile_id, dispatch_profile)
    };

    let Some(wt) = wt_path else {
        if let Some(flight) = verification {
            run_verification(run_id, task_id, flight, None).await;
        }
        return;
    };
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

    if let Some(flight) = verification {
        run_verification(run_id, task_id, flight, head_sha).await;
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

struct VerificationFlight(String);

impl VerificationFlight {
    fn active() -> &'static Mutex<HashSet<String>> {
        static ACTIVE: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
        ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
    }

    fn contains(dispatch_id: &str) -> bool {
        Self::active().lock().unwrap_or_else(|e| e.into_inner()).contains(dispatch_id)
    }

    fn reserve(dispatch_id: &str) -> Option<Self> {
        Self::active().lock().unwrap_or_else(|e| e.into_inner())
            .insert(dispatch_id.to_string()).then(|| Self(dispatch_id.to_string()))
    }

    fn for_report(run_id: &str, dispatch_id: &str) -> Option<Self> {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.runs.get(run_id)?;
        let dispatch = entry.dispatches.get(dispatch_id)?;
        let task = entry.tasks.get(&dispatch.task_id)?;
        (task.status == TaskStatus::Verifying
            && entry.latest_dispatch_for_task(&task.id).is_some_and(|d| d.id == dispatch_id))
            .then(|| Self::reserve(dispatch_id)).flatten()
    }
}

impl Drop for VerificationFlight {
    fn drop(&mut self) {
        Self::active().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}

fn restart_verifications(entry: &RunEntry) -> impl Iterator<Item = tokio::task::JoinHandle<()>> + '_ {
    entry.tasks.values().filter(|task| task.status == TaskStatus::Verifying).filter_map(|task| {
        let dispatch = entry.latest_dispatch_for_task(&task.id)
            .filter(|d| d.status == DispatchStatus::Reported)?;
        let flight = VerificationFlight::reserve(&dispatch.id)?;
        Some(tokio::spawn(run_verification(entry.run.id.clone(), task.id.clone(), flight, dispatch.head_sha.clone())))
    })
}

async fn run_verification(run_id: String, task_id: String, flight: VerificationFlight, head_sha: Option<String>) {
    use crate::orch::exec::{RunToExitInput, run_to_exit};

    let dispatch_id = flight.0.clone();
    let (verify_cmds, worktree, verify_timeout_min) = {
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
        if task.status != TaskStatus::Verifying
            || !entry.latest_dispatch_for_task(&task_id).is_some_and(|latest| latest.id == dispatch_id)
        {
            return;
        }
        let cwd = d.worktree.clone().unwrap_or_else(|| entry.run.repo.clone());
        (task.verify.clone(), cwd, entry.run.limits.verify_timeout_min)
    };

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
            Err(e) => crate::orch::exec::RunOutcome {
                exit_code: None, timed_out: false, tail: vec![e],
            },
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
            let run_id_c = run_id.clone();
            let task_id_c = task_id.clone();
            let dispatch_id_c = dispatch_id.clone();
            let cmd_c = cmd.clone();
            let now2 = unix_ms();
            let failure = classify_dispatch(&run_id, &dispatch_id, "verification_failed", &[]);
            let failure_c = failure.clone();
            let routed_failure = failure.as_ref().is_some_and(|f| f.routing == 1);
            let make_fail = move |g: &State| {
                let entry = g.runs.get(&run_id_c)
                    .ok_or_else(|| "not_found:run not found".to_string())?;
                Ok(with_failure_class(Cmd::TaskVerificationFailed {
                    run_id: run_id_c, task_id: task_id_c, dispatch_id: dispatch_id_c,
                    command: cmd_c, exit_code: exit_c, timed_out: timed_c,
                    tail: tail_str, msg_seq: entry.next_msg_seq(), now: now2,
                }, failure_c.as_ref()))
            };
            let committed = if routed_failure {
                commit_deferred_idle(make_fail, run_id.clone()).await
            } else {
                commit(make_fail, run_id.clone()).await
            };
            if let (CommitResult::Ok(_), Some(f)) = (committed, &failure) {
                send_follow_up(after_recorded_failure(&run_id, &dispatch_id, f, false).await).await;
            }
            return;
        }
    }

    let run_id_c = run_id.clone();
    let task_id_c = task_id.clone();
    let dispatch_id_c = dispatch_id.clone();
    let now = unix_ms();
    commit(move |g| {
        let entry = g.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        Ok(Cmd::TaskVerified {
            run_id: run_id_c, task_id: task_id_c, dispatch_id: dispatch_id_c,
            msg_seq: entry.next_msg_seq(), now,
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

impl DispatchWatch {
    fn observe_screen(&mut self, screen: &str, now_secs: u64) {
        if screen == self.last_screen {
            return;
        }
        self.last_screen = screen.to_string();
        self.screen_last_changed_secs = now_secs;
        self.consecutive_shell_ticks = 0;
        self.sent.remove("quiet");
    }
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
        autonomy::tick_all().await;
    }
}

async fn supervision_tick() {
    let now_secs = unix_ms() / 1000;
    let overdue: Vec<(String, String)> = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.values().filter(|e| !e.run.status.is_terminal()
            && e.messages.iter().any(|m| !m.acked && m.to == "coordinator"
                && unix_ms().saturating_sub(m.at) >= 60_000))
            .filter_map(|e| e.run.coordinator_tile.clone().map(|tile| (e.run.id.clone(), tile)))
            .collect()
    };
    for (run_id, tile) in overdue { recheck_coordinator(&run_id, &tile); }

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
        let (auto_stall, stall_pending, continued_secs) = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let Some(entry) = guard.runs.get(&run_id) else { continue };
            let Some(dispatch) = entry.dispatches.get(&dispatch_id) else { continue };
            let auto = entry.tasks.get(&dispatch.task_id)
                .is_some_and(|task| autonomy::can_handle_stall(entry, task, dispatch));
            let continuation = if auto { autonomy::stall_continuation(entry, &dispatch_id) } else { None };
            (auto, auto && autonomy::stall_pending(entry, &dispatch_id, continuation),
                continuation.map_or(0, |m| m.at / 1000))
        };
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
            if is_session_live || continued_secs != 0 {
                entry.ever_live = true;
            }
            let screen = worker::capture_worker_screen(&tile_id, 40);
            entry.observe_screen(&screen, now_secs);
            let quiet = now_secs.saturating_sub(entry.screen_last_changed_secs.max(continued_secs));
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
            running_secs: now_secs.saturating_sub(started_secs.max(continued_secs)),
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
                    let failure = classify_dispatch(&run_id, &dispatch_id, &reason, &tail_lines(&screen_text));
                    let failure_c = failure.clone();
                    let routed_failure = failure.as_ref().is_some_and(|f| f.routing == 1);
                    let make_fail = move |_: &State| {
                        Ok(with_failure_class(Cmd::DispatchUpdate {
                            run_id: run_id_c, dispatch_id: dispatch_id_c,
                            status: DispatchStatus::Failed,
                            summary: Some(summary_c),
                            now,
                        }, failure_c.as_ref()))
                    };
                    let committed = if routed_failure {
                        commit_deferred_idle(make_fail, run_id.clone()).await
                    } else {
                        commit(make_fail, run_id.clone()).await
                    };
                    let follow_up = match (committed, &failure) {
                        (CommitResult::Ok(_), Some(f)) => after_recorded_failure(&run_id, &dispatch_id, f, false).await,
                        _ => None,
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
                            seq: entry.next_msg_seq(),
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
                    send_follow_up(follow_up).await;
                }
                Finding::Timeout | Finding::Quiet => {
                    if fail_live_dispatch_on_quota(&run_id, &dispatch_id, &screen_text).await {
                        break;
                    }
                    if stall_pending {
                        break;
                    }
                    let tail = screen_text.lines().rev().take(20).collect::<Vec<_>>()
                        .into_iter().rev().collect::<Vec<_>>().join("\n");
                    let body_str = serde_json::json!({"reason": reason, "tail": tail}).to_string();
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
                            seq: entry.next_msg_seq(),
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
                    if auto_stall {
                        break;
                    }
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

    let Some(flight) = VerificationFlight::reserve(&dispatch_id) else {
        return err_resp("conflict", "verification is already running");
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

    tokio::spawn(run_verification(run_id, task_id, flight, head_sha));
    ok_resp(serde_json::json!({"started": true}))
}

async fn handle_conflict_resolve(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }

    let by = if v["by"] == "brain" { "brain" } else { "coordinator" };
    let merge_branch = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return err_resp("not_found", "task not found"),
        };
        let has_open_gate = entry.gates.values().any(|g| g.task_id == task_id && g.resolution.is_none());
        if task.status != TaskStatus::Blocked || has_open_gate
            || (by == "brain" && !autonomy::can_resolve_conflict(entry, task))
        {
            return err_resp("conflict", "task must be blocked by an integration conflict to resolve it");
        }
        let source = entry.dispatches.values()
            .filter(|d| d.task_id == task_id
                && d.status == DispatchStatus::Reported
                && d.outcome.as_deref() == Some("succeeded")
                && d.branch.is_some())
            .max_by_key(|d| d.attempt);
        match source {
            Some(d) => d.branch.clone().unwrap(),
            None => return err_resp("conflict", "no reported dispatch with a branch to merge"),
        }
    };

    let mut inner = serde_json::json!({
        "runId": run_id,
        "taskId": task_id,
        "by": by,
    });
    for key in ["profile", "model", "effort"] {
        if let Some(s) = v[key].as_str() {
            inner[key] = s.into();
        }
    }
    worker_start_inner(inner, Some(merge_branch)).await
}

async fn handle_retry(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }

    let by_retry = v["by"].as_str().unwrap_or("coordinator");

    if by_retry != "brain" {
        autonomy::clear_action_failure(&run_id, &task_id).await;
    }
    let routed_limit = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        match entry.tasks.get(&task_id) {
            Some(t) if t.routing == 1 && t.status == TaskStatus::Failed => {
                if by_retry != "user" && t.owner.as_deref() == Some("user") {
                    return err_resp("owned_by_user", "this task is owned by the user; only the user can retry it");
                }
                Some((wiring::retry_limit(entry, t), entry.latest_dispatch_for_task(&task_id).map(|d| d.id.clone())))
            }
            _ => None,
        }
    };
    if let Some((limit, retry_of)) = routed_limit {
        match limit {
            wiring::RetryLimit::QualityExhausted => {
                return err_resp("quality_attempts_exhausted", "quality attempts exhausted for this task");
            }
            wiring::RetryLimit::OperationalExhausted => {
                return enter_waiting(EnterWaitingInput {
                    run_id, task_id, dispatch_id: None, wait: wiring::operational_cap_wait(),
                }).await;
            }
            wiring::RetryLimit::Open => {}
        }
        let mut inner_body = serde_json::json!({"runId": run_id, "taskId": task_id, "by": by_retry});
        for key in ["profile", "model", "effort"] {
            if let Some(explicit) = v[key].as_str() {
                inner_body[key] = explicit.into();
            }
        }
        if let Some(stop_live) = v["stopLive"].as_bool() {
            inner_body["stopLive"] = stop_live.into();
        }
        if let Some(ro) = retry_of {
            inner_body["retryOf"] = ro.into();
        }
        return handle_worker_start(inner_body.to_string().as_bytes()).await;
    }


    let retry_of = {
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
        entry.latest_dispatch_for_task(&task_id).map(|d| d.id.clone())
    };
    let mut inner_body = serde_json::json!({"runId": run_id, "taskId": task_id, "by": by_retry});
    for key in ["profile", "model", "effort"] {
        if let Some(explicit) = v[key].as_str() {
            inner_body[key] = explicit.into();
        }
    }
    if let Some(ro) = retry_of {
        inner_body["retryOf"] = ro.into();
    }
    if by_retry == "brain" && v["stopLive"] == true {
        inner_body["stopLive"] = true.into();
    }
    handle_worker_start(inner_body.to_string().as_bytes()).await
}

fn build_report(entry: &RunEntry, integration_tip: Option<String>) -> Value {
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
                    "report": d.report,
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

    serde_json::json!({
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
    })
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

fn render_report_markdown(data: &Value, usage: &Value) -> String {
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
    out.push_str(&usage::markdown(usage));
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
                    if let Some(report) = d["report"].as_str() { out.push_str(&format!("\n{report}\n\n")); }
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

    let (report, usage) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(&run_id) else { return err_resp("not_found", "run not found") };
        (build_report(entry, int_tip), (format == "markdown").then(|| usage::RunUsage::snapshot(entry)))
    };
    let Some(usage) = usage else { return ok_resp(report) };
    match tokio::task::spawn_blocking(move || render_report_markdown(&report, &usage.read())).await {
        Ok(text) => ok_resp(serde_json::json!({"text": text})),
        Err(e) => err_resp("io", &e.to_string()),
    }
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
    if v["by"].as_str().unwrap_or("coordinator") != "brain" {
        autonomy::clear_action_failure(&run_id, &task_id).await;
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
    let branch_for_conflict = branch.clone();

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
            let result = commit(move |g| {
                let entry = g.runs.get(&run_id_c)
                    .ok_or_else(|| "not_found:run not found".to_string())?;
                Ok(Cmd::TaskIntegrate {
                    run_id: run_id_c,
                    task_id: task_id_c,
                    dispatch_id: dispatch_id_c,
                    integrated_sha: sha.clone(),
                    msg_seq: entry.next_msg_seq(),
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
            let branch_c = branch_for_conflict.clone();
            let tip_path = int_path.clone();
            let integration_tip = tokio::task::spawn_blocking(move || git::head_sha(&tip_path).unwrap_or_default())
                .await
                .unwrap_or_default();
            let now = unix_ms();
            let result = commit(move |g| {
                let entry = g.runs.get(&run_id_c)
                    .ok_or_else(|| "not_found:run not found".to_string())?;
                Ok(Cmd::TaskConflict {
                    run_id: run_id_c,
                    task_id: task_id_c,
                    files: files_c,
                    branch: branch_c,
                    integration_tip,
                    msg_seq: entry.next_msg_seq(),
                    now,
                })
            }, run_id.clone()).await;
            match result {
                CommitResult::Err(e) if v["by"] == "brain" => err_from_str(&e),
                _ => ok_resp(serde_json::json!({"conflict": true, "files": files})),
            }
        }
        Err(e) => err_resp("conflict", &e),
    }
}

async fn handle_worktree_clean(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() { return err_resp("bad_request", "runId required"); }

    let Some(worktree_lock) = run_worktree_lock(&run_id) else { return err_resp("not_found", "run not found") };
    let _worktree_guard = worktree_lock.lock().await;
    let (repo, base_sha, candidates) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let mut by_worktree = HashMap::<&str, &Dispatch>::new();
        for dispatch in entry.dispatches.values() {
            let Some(path) = dispatch.worktree.as_deref() else { continue };
            let selected = by_worktree.entry(path).or_insert(dispatch);
            if selected.status != DispatchStatus::Running
                && (dispatch.status == DispatchStatus::Running || dispatch.attempt > selected.attempt)
            {
                *selected = dispatch;
            }
        }
        let candidates: Vec<CleanCandidate> = by_worktree.into_iter()
            .filter_map(|(path, d)| Some(CleanCandidate {
                id: d.id.clone(),
                worktree: path.into(),
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

    if release_worker_sessions(&run_id, &removable).await {
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
async fn release_worker_sessions(run_id: &str, candidates: &[CleanCandidate]) -> bool {
    let mut released = false;
    for candidate in candidates {
        let Some(tile) = candidate.tile_id.as_deref() else { continue };
        if let Err(e) = save_worker_transcript(run_id, &candidate.id, tile) { eprintln!("[orch] transcript run {run_id} dispatch {}: {e}", candidate.id); }
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
    let options = CheckOptions {
        recipient,
        types,
        peek: v["peek"].as_bool().unwrap_or(false),
        decisions: v["decisions"].as_bool().unwrap_or(false),
    };
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

        let waiting = wait && tokio::time::Instant::now() < deadline;
        let result = tokio::task::block_in_place(|| {
            collect_ack_sync(&run_id, &options, &base, waiting)
        });

        match result {
            Err(e) => return err_from_str(&e),
            Ok((msgs, routine)) if !msgs.is_empty() || !waiting => {
                return routine.response(msgs, options.decisions);
            }
            Ok(_) => {}
        }

        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let _ = tokio::time::timeout(remaining, n).await;
    }
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
    worker_start_inner(v, None).await
}

async fn worker_start_inner(v: Value, merge_branch: Option<String>) -> String {
    let run_id = v["runId"].as_str().unwrap_or("").to_string();
    let task_id = v["taskId"].as_str().unwrap_or("").to_string();
    if run_id.is_empty() || task_id.is_empty() {
        return err_resp("bad_request", "runId and taskId required");
    }
    let explicit_profile: Option<String> = v["profile"].as_str().map(str::to_string);
    let explicit_model: Option<String> = v["model"].as_str().map(str::to_string);
    let retry_of: Option<String> = v["retryOf"].as_str().map(str::to_string);
    let by = v["by"].as_str().unwrap_or("coordinator").to_string();
    let explicit_effort: Option<Effort> = match v.get("effort").and_then(|e| e.as_str()).filter(|s| !s.is_empty()) {
        Some(s) => match Effort::from_str_opt(s) {
            Some(eff) => Some(eff),
            None => return err_resp("bad_effort", &format!("invalid effort: {s}")),
        },
        None => None,
    };

    let (route, task_role) = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = match guard.runs.get(&run_id) {
            Some(e) => e,
            None => return err_resp("not_found", "run not found"),
        };
        let task = match entry.tasks.get(&task_id) {
            Some(t) => t,
            None => return err_resp("not_found", "task not found"),
        };
        if by != "user" && merge_branch.is_none() && task.owner.as_deref() == Some("user") {
            return err_resp("owned_by_user", "this task is owned by the user; only the user can dispatch it");
        }
        let task_role = task.role.clone();
        let route = if task.routing == 1 {
            let states = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
            match wiring::plan_dispatch(plan_input(entry, &task_id, &v, &by), &states) {
                Err(e) => return err_from_str(&e),
                Ok(wiring::DispatchPlan::Wait(wait)) => StartRoute::Wait(wait),
                Ok(wiring::DispatchPlan::Dispatch(p)) => StartRoute::Routed(p),
            }
        } else {
            let corrections = entry.dispatches.values().filter(|d| d.task_id == task_id && d.corrective_retry).count() as u32;
            let attempt = task.attempts.saturating_sub(corrections + task.operational_attempts) + 1;
            let latest_profile = entry.latest_dispatch_for_task(&task_id).and_then(|d| d.profile.clone());
            let latest_model = entry.latest_dispatch_for_task(&task_id).and_then(|d| d.model.clone());
            let correction = wiring::corrective_source(entry, &task_id)
                .filter(|_| explicit_profile.is_none() && explicit_model.is_none() && merge_branch.is_none());
            let (p, m) = if let Some(previous) = correction {
                (previous.profile.clone().unwrap_or_default(), previous.model.clone())
            } else {
                pick_escalation(EscalationInput {
                    escalation: &task.escalation,
                    attempt,
                    explicit_profile: explicit_profile.as_deref(),
                    explicit_model: explicit_model.as_deref(),
                    task_profile: task.profile.as_deref(),
                    latest_profile: latest_profile.as_deref(),
                    latest_model: latest_model.as_deref(),
                })
            };
            if p.is_empty() {
                return err_resp("bad_request", "profile required (no escalation list and no task profile)");
            }
            StartRoute::Legacy(p, m)
        };
        (route, task_role)
    };
    let (profile_id, model, routed) = match route {
        StartRoute::Wait(wait) => {
            if by == "brain" && merge_branch.is_some() {
                return err_resp("conflict", &wait.reason);
            }
            return enter_waiting(EnterWaitingInput { run_id, task_id, dispatch_id: None, wait }).await;
        }
        StartRoute::Routed(p) => (p.profile.clone(), p.model.clone(), Some(p)),
        StartRoute::Legacy(p, m) => (p, m, None),
    };
    let worktree_mode = v["worktree"].as_str().unwrap_or("auto").to_string();
    let stop_live = v["stopLive"].as_bool().unwrap_or(false);

    let Some(base) = crate::panorama_dir() else {
        return err_resp("io", "no panorama dir");
    };

    let effort = explicit_effort.or_else(|| settings::load(&base).effort_for(task_role, &profile_id, model.as_deref()));

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
    let merge_branch_c = merge_branch.clone();
    let account = routed.as_ref().and_then(|_| account_for_profile(&profile));
    let mut extra_env = resolve_env(&profile.env, Some(&base));
    let native_home = worker::pin_native_home(&profile.exe, &tile_id, &mut extra_env);
    extra_env.push(("PANORAMA_RUN_ID".into(), run_id.clone()));
    let routed_c = routed.clone();
    let v_c = v.clone();
    let by_c = by.clone();

    let result = commit(move |g| {
        let entry = g.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        let task = entry.tasks.get(&task_id_c)
            .ok_or_else(|| "not_found:task not found".to_string())?;
        if by_c == "brain" && merge_branch_c.is_some() && !autonomy::can_resolve_conflict(entry, task) {
            return Err("conflict:task no longer eligible for automatic conflict resolution".into());
        }
        let attempt = task.attempts + 1;
        let dispatch_id = format!("{run_id_c}.{task_id_c}.{attempt}");
        let mut fields = match &routed_c {
            Some(expected) => routed_dispatch_fields(RoutedFieldsInput {
                entry, task_id: &task_id_c, request: &v_c, by: &by_c, expected, account,
            })?,
            None => serde_json::json!({}),
        };
        if let Some(previous) = wiring::corrective_source(entry, &task_id_c)
            .filter(|d| d.profile.as_ref() == Some(&profile_id_c) && d.model == model_c && merge_branch_c.is_none())
        {
            if fields["chainIndex"].is_null() {
                fields["chainIndex"] = serde_json::json!(previous.chain_index);
            }
            fields["correctiveRetry"] = true.into();
        }
        fields["nativeHome"] = serde_json::json!(native_home);
        if let Some(eff) = effort {
            fields["effort"] = serde_json::json!(eff);
        }
        let cmd = Cmd::Dispatch {
            run_id: run_id_c.clone(),
            id: dispatch_id,
            task_id: task_id_c.clone(),
            attempt,
            retry_of: retry_of_c,
            profile: Some(profile_id_c.clone()),
            model: model_c,
            tile_id: Some(tile_id_c.clone()),
            owner: owner_c,
            merge_branch: merge_branch_c,
            now,
        };
        Ok(cmd.with_fields(fields))
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

    extra_env.push(("PANORAMA_DISPATCH_ID".into(), dispatch_id.clone()));
    let sd_input = StartDispatchInput {
        run_id: run_id.clone(),
        task_id: task_id.clone(),
        dispatch_id: dispatch_id.clone(),
        tile_id: tile_id.clone(),
        profile,
        model,
        effort,
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
        extra_env,
    };
    if !cfg!(test) {
        starting().lock().unwrap_or_else(|e| e.into_inner()).insert(dispatch_id.clone());
        tokio::spawn(async move {
            start_dispatch(sd_input).await;
        });
    }

    let dispatch_json = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.dispatches.get(&dispatch_id))
            .and_then(|d| serde_json::to_value(d).ok())
            .unwrap_or(serde_json::json!({"id": dispatch_id}))
    };
    ok_resp(serde_json::json!({"dispatch": dispatch_json, "starting": true}))
}

enum StartRoute {
    Legacy(String, Option<String>),
    Routed(wiring::PlannedDispatch),
    Wait(WaitRequest),
}

fn plan_input<'a>(entry: &'a reducer::RunEntry, task_id: &'a str, request: &'a Value, by: &str) -> wiring::PlanInput<'a> {
    wiring::PlanInput {
        entry,
        task_id,
        explicit_profile: request["profile"].as_str(),
        explicit_model: request["model"].as_str(),
        by: wiring::dispatch_by(by),
        resumed: request["resume"].as_bool().unwrap_or(false),
        now: unix_ms() as i64,
    }
}

struct RoutedFieldsInput<'a> {
    entry: &'a reducer::RunEntry,
    task_id: &'a str,
    request: &'a Value,
    by: &'a str,
    expected: &'a wiring::PlannedDispatch,
    account: Option<String>,
}

fn routed_dispatch_fields(input: RoutedFieldsInput) -> Result<Value, String> {
    let RoutedFieldsInput { entry, task_id, request, by, expected, account } = input;
    let states = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
    match wiring::plan_dispatch(plan_input(entry, task_id, request, by), &states)? {
        wiring::DispatchPlan::Dispatch(p) if p.profile == expected.profile && p.model == expected.model => {
            Ok(wiring::dispatch_fields(&p, account))
        }
        _ => Err("conflict:routing changed while dispatching; retry".into()),
    }
}

fn account_for_profile(profile: &crate::orch::profiles::Profile) -> Option<String> {
    if profile.provider {
        return None;
    }
    crate::accounts::default_account_id(&profile.exe)
}

struct EnterWaitingInput {
    run_id: String,
    task_id: String,
    dispatch_id: Option<String>,
    wait: WaitRequest,
}

async fn send_coordinator_notice(run_id: String, task_id: String, dispatch_id: Option<String>, msg_type: &'static str, body: String) {
    let run_id_c = run_id.clone();
    commit(move |g| {
        let entry = g.runs.get(&run_id_c)
            .ok_or_else(|| "not_found:run not found".to_string())?;
        wiring::notice_cmd(entry, wiring::NoticeInput {
            run_id: &run_id_c,
            task_id: &task_id,
            dispatch_id: dispatch_id.as_deref(),
            msg_type,
            body,
            now: unix_ms(),
        }).ok_or_else(|| "conflict:duplicate notice".to_string())
    }, run_id).await;
}

async fn commit_wait(run_id: &str, task_id: &str, wait: WaitRequest) -> Result<String, String> {
    let body = wiring::task_waiting_body(&wait);
    let (run_c, task_c) = (run_id.to_string(), task_id.to_string());
    let now = unix_ms();
    match commit(move |_| Ok(Cmd::TaskWait { run_id: run_c, task_id: task_c, wait, now }), run_id.to_string()).await {
        CommitResult::Ok(_) => Ok(body),
        CommitResult::Stale => Err("stale:task wait stale".into()),
        CommitResult::Err(e) => Err(e),
    }
}

async fn commit_wait_and_notify(input: EnterWaitingInput) -> Result<(), String> {
    let EnterWaitingInput { run_id, task_id, dispatch_id, wait } = input;
    let body = commit_wait(&run_id, &task_id, wait).await?;
    let notice_dispatch = dispatch_id.or_else(|| {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(&run_id)
            .and_then(|e| e.latest_dispatch_for_task(&task_id))
            .map(|d| d.id.clone())
    });
    send_coordinator_notice(run_id, task_id, notice_dispatch, "task_waiting", body).await;
    Ok(())
}

async fn enter_waiting(input: EnterWaitingInput) -> String {
    let summary = serde_json::json!({
        "kind": input.wait.kind,
        "resumeAt": input.wait.resume_at,
        "reason": input.wait.reason,
        "candidate": input.wait.candidate,
    });
    match commit_wait_and_notify(input).await {
        Ok(()) => ok_resp(serde_json::json!({"waiting": summary})),
        Err(e) => err_from_str(&e),
    }
}

#[derive(Clone)]
struct DispatchFailure {
    classification: failure::Classification,
    routing: u32,
}

struct FollowUp {
    run_id: String,
    task_id: String,
    dispatch_id: String,
    notices: Vec<(&'static str, String)>,
}

fn classify_dispatch(run_id: &str, dispatch_id: &str, reason_code: &str, tail: &[String]) -> Option<DispatchFailure> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let entry = guard.runs.get(run_id)?;
    let dispatch = entry.dispatches.get(dispatch_id)?;
    let routing = entry.tasks.get(&dispatch.task_id)?.routing;
    let classification = wiring::classify_failure(dispatch.profile.as_deref(), reason_code, tail, unix_ms() as i64);
    Some(DispatchFailure { classification, routing })
}

fn with_failure_class(cmd: Cmd, failure: Option<&DispatchFailure>) -> Cmd {
    let Some(failure) = failure else { return cmd };
    let field = if failure.routing == 1 { "routedFailureClass" } else { "failureClass" };
    cmd.with_fields(serde_json::json!({(field): failure.classification.class}))
}

fn dispatch_screen_tail(run_id: &str, dispatch_id: &str) -> Vec<String> {
    let tile = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(run_id)
            .and_then(|e| e.dispatches.get(dispatch_id))
            .and_then(|d| d.tile_id.clone())
    };
    tile.map(|t| tail_lines(&worker::capture_worker_screen(&t, 40))).unwrap_or_default()
}

fn tail_lines(text: &str) -> Vec<String> {
    text.lines().map(str::to_string).collect()
}

async fn after_recorded_failure(run_id: &str, dispatch_id: &str, failure: &DispatchFailure, live: bool) -> Option<FollowUp> {
    if failure.routing != 1 {
        return None;
    }
    let follow_up = async {
        let class = failure.classification.class;
        let dispatch = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.runs.get(run_id)?;
            let dispatch = entry.dispatches.get(dispatch_id)?;
            if dispatch.failure_class != Some(class)
                || !entry.latest_dispatch_for_task(&dispatch.task_id).is_some_and(|latest| latest.id == dispatch_id)
            {
                return None;
            }
            dispatch.clone()
        };
        let task_id = dispatch.task_id.clone();
        let now = unix_ms();
        let (after, snapshot) = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let mut states = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
            let marked = wiring::mark_candidate(&mut states, &dispatch, &failure.classification, now as i64);
            let snapshot = marked.then(|| snapshot_candidates(&states));
            let entry = guard.runs.get(run_id)?;
            let after = wiring::after_failure(wiring::AfterFailureInput {
                entry, states: &states, dispatch_id, live, now: now as i64,
            });
            (after, snapshot)
        };
        if let Some(snapshot) = snapshot {
            if let Err(e) = persist_candidates(&snapshot) {
                eprintln!("[orch] candidate states not persisted: {e}");
            }
        }
        let mut notices: Vec<(&'static str, String)> = Vec::new();
        if let Some(wait) = after.wait {
            if let Ok(body) = commit_wait(run_id, &task_id, wait).await {
                notices.push(("task_waiting", body));
            }
        } else if let Some(suggested) = after.suggested.filter(|_| class.counter() == FailureCounter::Operational) {
            notices.push(("operational_failure", wiring::operational_failure_body(class, dispatch_id, &suggested)));
        }
        Some(FollowUp { run_id: run_id.to_string(), task_id, dispatch_id: dispatch_id.to_string(), notices })
    }.await;
    sync_idle(run_id).await;
    follow_up
}

async fn send_follow_up(follow_up: Option<FollowUp>) {
    let Some(FollowUp { run_id, task_id, dispatch_id, notices }) = follow_up else { return };
    for (msg_type, body) in notices {
        send_coordinator_notice(run_id.clone(), task_id.clone(), Some(dispatch_id.clone()), msg_type, body).await;
    }
}

async fn fail_live_dispatch_on_quota(run_id: &str, dispatch_id: &str, screen: &str) -> bool {
    let tail = tail_lines(screen);
    let Some(failure) = classify_dispatch(run_id, dispatch_id, "", &tail) else { return false };
    if failure.routing != 1 || !wiring::is_live_quota_class(failure.classification.class) {
        return false;
    }
    let (run_c, dispatch_c) = (run_id.to_string(), dispatch_id.to_string());
    let summary = format!("worker blocked: {}", failure.classification.source);
    let now = unix_ms();
    let failure_c = failure.clone();
    let updated = commit_deferred_idle(move |_| {
        Ok(with_failure_class(Cmd::DispatchUpdate {
            run_id: run_c, dispatch_id: dispatch_c, status: DispatchStatus::Failed, summary: Some(summary), now,
        }, Some(&failure_c)))
    }, run_id.to_string()).await;
    if !matches!(updated, CommitResult::Ok(_)) {
        return false;
    }
    send_follow_up(after_recorded_failure(run_id, dispatch_id, &failure, true).await).await;
    true
}

fn clear_candidate_on_success(run_id: &str, dispatch_id: &str) {
    let key = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(dispatch) = guard.runs.get(run_id).and_then(|e| e.dispatches.get(dispatch_id)) else { return };
        let Some(task) = guard.runs.get(run_id).and_then(|e| e.tasks.get(&dispatch.task_id)) else { return };
        if task.routing != 1 {
            return;
        }
        wiring::dispatch_key(dispatch)
    };
    let Some(key) = key else { return };
    let snapshot = {
        let mut states = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
        (states.clear_on_success(&key) > 0).then(|| snapshot_candidates(&states))
    };
    if let Some(snapshot) = snapshot {
        if let Err(e) = persist_candidates(&snapshot) {
            eprintln!("[orch] candidate states not persisted: {e}");
        }
    }
}

struct StartDispatchInput {
    run_id: String,
    task_id: String,
    dispatch_id: String,
    tile_id: String,
    profile: crate::orch::profiles::Profile,
    model: Option<String>,
    effort: Option<Effort>,
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
    extra_env: Vec<(String, String)>,
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
    let parsed_body: Value = serde_json::from_str(&body).unwrap_or_default();
    let reason_code = parsed_body["reason"].as_str().unwrap_or("").to_string();
    let tail = tail_lines(parsed_body["tail"].as_str().unwrap_or(""));
    let failure = classify_dispatch(&run_id, &dispatch_id, &reason_code, &tail);
    let failure_c = failure.clone();
    let routed_failure = failure.as_ref().is_some_and(|f| f.routing == 1);
    let make_fail = move |_: &State| {
        Ok(with_failure_class(Cmd::DispatchUpdate {
            run_id: run_id2, dispatch_id: dispatch_id2,
            status: DispatchStatus::Failed,
            summary: Some(summary2),
            now,
        }, failure_c.as_ref()))
    };
    let committed = if routed_failure {
        commit_deferred_idle(make_fail, run_id.clone()).await
    } else {
        commit(make_fail, run_id.clone()).await
    };
    let follow_up = match (committed, &failure) {
        (CommitResult::Ok(_), Some(f)) => after_recorded_failure(&run_id, &dispatch_id, f, false).await,
        _ => None,
    };

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
    send_follow_up(follow_up).await;
}

struct Carry {
    start_ref: String,
    handoff: String,
}

async fn stop_worker_with<L, K>(tile_id: &str, is_live: L, kill: K) -> bool
where
    L: Fn(&str) -> bool,
    K: Fn(&str),
{
    for _ in 0..10 {
        if !is_live(tile_id) { return true; }
        kill(tile_id);
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    !is_live(tile_id)
}

async fn ensure_worker_stopped(tile_id: &str) -> bool {
    stop_worker_with(
        tile_id,
        |t| crate::lookup_live(t).is_some(),
        |t| if let Some(session) = crate::lookup_live(t) { crate::kill_session(&session) },
    ).await
}

async fn prepare_carry(run_id: &str, dispatch_id: &str, fallback_start_ref: String) -> Result<Option<Carry>, String> {
    prepare_carry_with(run_id, dispatch_id, fallback_start_ref, |tile| async move { ensure_worker_stopped(&tile).await }).await
}

async fn prepare_carry_with<F, Fut>(run_id: &str, dispatch_id: &str, fallback_start_ref: String, stop: F) -> Result<Option<Carry>, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let previous = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(run_id) else { return Ok(None) };
        let Some(next) = entry.dispatches.get(dispatch_id) else { return Ok(None) };
        carry::previous_for_carry(entry, next)
    };
    let Some(previous) = previous else { return Ok(None) };
    if let Some(tile) = previous.tile_id.clone() {
        if !stop(tile).await {
            return Err("previous worker still live".into());
        }
    }
    let previous_c = previous.clone();
    let checkpoint = tokio::task::spawn_blocking(move || {
        carry::checkpoint_previous(carry::CheckpointInput { previous: &previous_c, fallback_start_ref: &fallback_start_ref })
    }).await.map_err(|e| format!("checkpoint panic: {e}"))??;
    Ok(checkpoint.map(|cp| Carry { handoff: carry::handoff_block(&previous, &cp), start_ref: cp.head }))
}

fn run_worktree_lock(run_id: &str) -> Option<Arc<tokio::sync::Mutex<()>>> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    Some(guard.runs.get(run_id)?.worktree_lock.clone())
}

async fn stop_native_worker(tile_id: &str) -> bool {
    let session = crate::sessions().lock().unwrap().get(tile_id).cloned();
    let Some(session) = session else { return true };
    if !session.exited.load(std::sync::atomic::Ordering::Acquire) {
        crate::kill_session(&session);
    }
    let deadline = std::time::Instant::now() + crate::RUN_KILL_CONFIRM;
    while !session.exited.load(std::sync::atomic::Ordering::Acquire)
        || session.kill_pending.load(std::sync::atomic::Ordering::Acquire)
    {
        if std::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    true
}

fn native_resume_id<'a>(previous: &'a Dispatch, next: &Dispatch, exe: &str) -> Option<&'a str> {
    let eligible = (next.corrective_retry && previous.retry_same_entry)
        || (next.trigger == Some(Trigger::Resume) && previous.operational_wait);
    if !eligible || previous.profile != next.profile || previous.model != next.model
        || previous.account != next.account || previous.native_home != next.native_home
        || next.merge_branch.is_some() || previous.merge_branch.is_some()
        || (next.worktree.is_some() && previous.worktree != next.worktree)
    {
        return None;
    }
    let session = previous.native_session.as_ref()?;
    let home = std::path::Path::new(previous.native_home.as_deref()?);
    if session.agent != exe || !worker::valid_session_id(&session.id) {
        return None;
    }
    crate::native_worker_transcript(home, exe, &session.id, previous.worktree.as_deref()?)?;
    Some(&session.id)
}

async fn prepare_native_resume(repo: &str, run_id: &str, next: &Dispatch, exe: &str) -> Option<Dispatch> {
    let previous = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(run_id)?.dispatches.values()
            .find(|d| d.task_id == next.task_id && d.attempt.checked_add(1) == Some(next.attempt))
            .cloned()?
    };
    let repo = repo.to_string();
    let next = next.clone();
    let exe = exe.to_string();
    let previous = tokio::task::spawn_blocking(move || {
        native_resume_id(&previous, &next, &exe)?;
        let path = previous.worktree.as_deref()?;
        let branch = previous.branch.as_deref()?;
        if !git::can_adopt_worktree(&repo, path, branch) {
            return None;
        }
        Some(previous)
    }).await.ok()??;
    if let Some(tile) = previous.tile_id.as_deref() {
        if !stop_native_worker(tile).await {
            return None;
        }
    }
    Some(previous)
}

async fn record_worker_worktree(run_id: &str, dispatch_id: &str, path: &str, branch: &str, start_ref: Option<String>) -> CommitResult {
    let (run, dispatch, worktree, branch) = (run_id.into(), dispatch_id.into(), path.into(), branch.into());
    commit(move |_| {
        let cmd = Cmd::DispatchSetWorktree {
            run_id: run, dispatch_id: dispatch, worktree, branch, now: unix_ms(),
        };
        Ok(match start_ref {
            Some(r) => cmd.with_fields(serde_json::json!({"startRef": r})),
            None => cmd,
        })
    }, run_id.into()).await
}

async fn start_dispatch(input: StartDispatchInput) {
    let StartDispatchInput {
        run_id, task_id, dispatch_id, tile_id, profile, model, effort, base, cli_path,
        worktree_mode, base_cwd, run_base_sha, run_setup, task_has_deps, repo,
        run_title, task_clone, dispatch_clone, deps_results, extra_env,
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

    let resolves_conflict = dispatch_clone.merge_branch.is_some();
    let use_worktree = worktree_mode != "none" && run_base_sha.is_some();
    let mut resume = None;
    if use_worktree && !resolves_conflict {
        if let Some(lock) = run_worktree_lock(&run_id) {
            let _guard = lock.lock().await;
            if let Some(previous) = prepare_native_resume(&repo, &run_id, &dispatch_clone, &profile.exe).await {
                if let Some((path, branch)) = previous.worktree.as_deref().zip(previous.branch.as_deref()) {
                    if matches!(record_worker_worktree(&run_id, &dispatch_id, path, branch, previous.start_ref.clone()).await, CommitResult::Ok(_)) {
                        resume = Some(previous);
                    }
                }
            }
        }
    }
    loop {
        if !is_running() { break; }

        let mut merge_note: Option<String> = None;
        let mut handoff_text: Option<String> = None;
        let cwd = if let Some(path) = resume.as_ref().and_then(|d| d.worktree.clone()) {
            path
        } else if use_worktree {
            let base_sha_val = run_base_sha.clone().unwrap_or_default();
            let int_branch = git::integration_branch(&run_id);
            let repo_c = repo.clone();
            let int_br_c = int_branch.clone();
            let base_for_wt = tokio::task::spawn_blocking(move || {
                if task_has_deps || resolves_conflict {
                    git::rev_parse(&repo_c, &int_br_c).unwrap_or(base_sha_val)
                } else {
                    base_sha_val
                }
            }).await.unwrap_or_else(|_| run_base_sha.clone().unwrap_or_default());

            let carried = match prepare_carry(&run_id, &dispatch_id, base_for_wt.clone()).await {
                Ok(c) => c,
                Err(e) => {
                    bail_fail!(
                        format!("checkpoint carry failed: {e}"),
                        "worker_failed".into(),
                        serde_json::json!({"reason":"worktree_failed","detail":e}).to_string()
                    );
                }
            };
            let (base_for_wt, start_ref, handoff) = match carried {
                Some(c) => (c.start_ref.clone(), Some(c.start_ref), Some(c.handoff)),
                None => (base_for_wt, None, None),
            };
            handoff_text = handoff;

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

            if !matches!(record_worker_worktree(&run_id, &dispatch_id, &wt_path, &wt_branch, start_ref).await, CommitResult::Ok(_)) {
                bail_fail!(
                    "worktree record failed".into(),
                    "worker_failed".into(),
                    serde_json::json!({"reason":"worktree_failed","detail":"could not record worker worktree"}).to_string()
                );
            }

            if let Some(setup_cmd) = run_setup.clone() {
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

            if let Some(source_branch) = dispatch_clone.merge_branch.clone() {
                if !is_running() { remove_starting(); return; }
                let wt_merge = wt_path.clone();
                let branch_merge = source_branch.clone();
                let merged = tokio::task::spawn_blocking(move || git::merge_no_commit(&wt_merge, &branch_merge)).await;
                let conflicted = match merged {
                    Ok(Ok(files)) => files,
                    Ok(Err(e)) => {
                        bail_fail!(
                            format!("conflict merge failed: {e}"),
                            "worker_failed".into(),
                            serde_json::json!({"reason":"merge_failed","detail":e}).to_string()
                        );
                    }
                    Err(e) => {
                        bail_fail!(
                            "conflict merge: spawn failed".into(),
                            "worker_failed".into(),
                            serde_json::json!({"reason":"merge_failed","detail":format!("spawn panic: {e}")}).to_string()
                        );
                    }
                };
                let integrated_titles: Vec<String> = {
                    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                    guard.runs.get(&run_id).map(|e| {
                        let mut tasks: Vec<&Task> = e.tasks.values()
                            .filter(|t| t.integrated_sha.is_some() && t.id != task_id)
                            .collect();
                        tasks.sort_by_key(|t| t.id.trim_start_matches('t').parse::<u32>().unwrap_or(u32::MAX));
                        tasks.into_iter().map(|t| t.title.clone()).collect()
                    }).unwrap_or_default()
                };
                merge_note = Some(worker::conflict_brief(&source_branch, &conflicted, &integrated_titles));
            }

            wt_path
        } else {
            base_cwd.clone()
        };

        if !is_running() { remove_starting(); return; }

        let dispatch_for_brief = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs.get(&run_id)
                .and_then(|e| e.dispatches.get(&dispatch_id))
                .cloned()
                .unwrap_or_else(|| dispatch_clone.clone())
        };

        let brief_path = match write_brief(BriefInput {
            base: &base,
            run_id: &run_id,
            dispatch_id: &dispatch_id,
            task: &task_clone,
            dispatch: &dispatch_for_brief,
            merge_note: merge_note.as_deref(),
            deps_results: &deps_results,
            cli_path: &cli_path,
            cwd: &cwd,
            handoff: handoff_text.as_deref(),
            correction: resume.as_ref(),
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
            match crate::orch::worker::write_worker_settings(&brief_path) {
                Ok(p) => Some(p.to_string_lossy().into_owned()),
                Err(e) => {
                    eprintln!("[orch] {dispatch_id}: worker permission settings skipped: {e}");
                    None
                }
            }
        } else {
            None
        };

        if !is_running() { remove_starting(); return; }

        let launch = WorkerLaunch {
            tile_id: &tile_id,
            cwd: &cwd,
            group: Some(TileGroup { id: &run_id, title: &run_title }),
            profile: &profile,
            model: model.as_deref(),
            effort,
            extra_env: extra_env.clone(),
            brief_path: &brief_path,
            settings_path,
            resume_id: resume.as_ref().and_then(|d| d.native_session.as_ref()).map(|session| session.id.as_str()),
        };
        if let Err(e) = spawn_worker(launch).await {
            if resume.take().is_some() && stop_native_worker(&tile_id).await {
                crate::clear_binding_session(&tile_id);
                continue;
            }
            bail_fail!(
                format!("spawn failed: {e}"),
                "worker_failed".into(),
                serde_json::json!({"reason":"spawn_failed","detail":e}).to_string()
            );
        }
        break;
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
    let mut dispatch_val = serde_json::to_value(dispatch).unwrap_or(serde_json::json!({}));
    if let Some(late) = entry.late_reports.get(dispatch_id) {
        dispatch_val["lateReports"] = serde_json::json!(late);
    }
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
    let text = if v["all"].as_bool().unwrap_or(false) {
        match worker::capture_worker_history(&tile_id) { Ok(text) => text, Err(e) => return err_resp("io", &e) }
    } else { capture_worker_screen(&tile_id, lines) };
    ok_resp(serde_json::json!({"text": text}))
}

fn transcript_path(run_id: &str, dispatch_id: &str) -> Result<std::path::PathBuf, String> {
    if [run_id, dispatch_id].iter().any(|id| id.is_empty() || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.') || *id == "." || *id == "..") {
        return Err("invalid run or dispatch id".into());
    }
    let base = crate::panorama_dir().ok_or("no panorama dir")?;
    Ok(base.join("orch").join("runs").join(run_id).join("transcripts").join(format!("{dispatch_id}.txt")))
}

fn cap_worker_transcript(text: &str) -> String {
    const CAP: usize = 2 * 1024 * 1024;
    if text.len() <= CAP { return text.to_owned(); }
    let mut start = text.len() - CAP;
    loop {
        while !text.is_char_boundary(start) { start += 1; }
        let header = format!("[transcript truncated: {start} bytes dropped]\n");
        if header.len() + text.len() - start <= CAP { break; }
        start += header.len() + text.len() - start - CAP;
    }
    if let Some(newline) = text[start..].find('\n') {
        if start + newline + 1 < text.len() { start += newline + 1; }
    }
    format!("[transcript truncated: {start} bytes dropped]\n{}", &text[start..])
}

fn save_worker_transcript(run_id: &str, dispatch_id: &str, tile_id: &str) -> Result<(), String> {
    if !crate::sessions().lock().unwrap().contains_key(tile_id) { return Ok(()); }
    let text = worker::capture_worker_history(tile_id)?;
    let path = transcript_path(run_id, dispatch_id)?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, cap_worker_transcript(&text)).map_err(|e| e.to_string())
}

fn handle_worker_transcript(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let run_id = v["runId"].as_str().unwrap_or("");
    let dispatch_id = v["dispatchId"].as_str().unwrap_or("");
    let tile = dispatch_tile(run_id, dispatch_id).ok();
    let text = if let Some(tile) = tile.filter(|t| crate::lookup_live(t).is_some()) {
        worker::capture_worker_history(&tile)
    } else {
        transcript_path(run_id, dispatch_id).and_then(|p| std::fs::read_to_string(p).map_err(|e| e.to_string()))
    };
    match text {
        Ok(text) => {
            let text = if let Some(n) = v["tail"].as_u64() {
                let lines: Vec<_> = text.lines().collect();
                lines[lines.len().saturating_sub(n as usize)..].join("\n")
            } else { text };
            ok_resp(serde_json::json!({"text": text}))
        }
        Err(e) => err_resp("io", &e),
    }
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
    if let Some(bytes) = v["keysBytes"].as_array() {
        let bytes: Option<Vec<u8>> = bytes.iter().map(|b| b.as_u64().and_then(|n| u8::try_from(n).ok())).collect();
        let Some(bytes) = bytes else { return err_resp("bad_request", "invalid keysBytes"); };
        crate::write_input_bytes(&session, &bytes);
    } else {
        crate::write_input(&session, text);
    }
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
    }, run_id.clone()).await;
    match result {
        CommitResult::Ok(_) | CommitResult::Stale => {}
        CommitResult::Err(e) => return Err(e),
    }
    stop_worker_session(&run_id, &dispatch_id, &tile_id);
    Ok(())
}

fn stop_worker_session(run_id: &str, dispatch_id: &str, tile_id: &str) {
    if !tile_id.is_empty() {
        if let Err(e) = save_worker_transcript(run_id, dispatch_id, tile_id) { eprintln!("[orch] transcript run {run_id} dispatch {dispatch_id}: {e}"); }
        if let Some(s) = crate::sessions().lock().unwrap().get(tile_id).cloned() {
            crate::kill_session(&s);
        }
    }
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
    stop_worker_session(&run_id, &dispatch_id, &tile_id);
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
            verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now,
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
        if let Err(e) = save_worker_transcript(&run_id, &dispatch_id, &tile_id) { eprintln!("[orch] transcript run {run_id} dispatch {dispatch_id}: {e}"); }
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
        "roleEfforts": s.role_efforts,
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
                if let Some(role_map) = s.role_efforts.get_mut(*role_name) {
                    role_map.retain(|k, _| chain.contains(k));
                    if role_map.is_empty() {
                        s.role_efforts.remove(*role_name);
                    }
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
    if let Some(efforts_obj) = v.get("roleEfforts").filter(|r| r.is_object()) {
        let valid_roles = ["explore", "implement", "test", "review", "mechanical"];
        for (role_name, entries_val) in efforts_obj.as_object().unwrap() {
            if !valid_roles.contains(&role_name.as_str()) {
                return err_resp("bad_role", &format!("unknown role: {role_name}"));
            }
            let Some(entries_map) = entries_val.as_object() else {
                return err_resp("bad_effort", &format!("roleEfforts.{role_name} must be an object"));
            };
            for (entry_key, eff_val) in entries_map {
                if eff_val.is_null() || eff_val.as_str() == Some("") {
                    if let Some(role_map) = s.role_efforts.get_mut(role_name) {
                        role_map.remove(entry_key);
                    }
                } else if let Some(s_eff) = eff_val.as_str() {
                    let Some(eff) = Effort::from_str_opt(s_eff) else {
                        return err_resp("bad_effort", &format!("invalid effort: {s_eff}"));
                    };
                    s.role_efforts.entry(role_name.clone()).or_default().insert(entry_key.clone(), eff);
                } else {
                    return err_resp("bad_effort", "effort must be a string or null");
                }
            }
            if s.role_efforts.get(role_name).is_some_and(|m| m.is_empty()) {
                s.role_efforts.remove(role_name);
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
                "supportsEffort": profile.effort_arg.is_some(),
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
                "supportsEffort": profile.effort_arg.is_some(),
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

    match tokio::task::spawn_blocking(move || ai::run_oneshot(ai::OneShotOpts { action: "commitMessage", provider, model, prompt })).await {
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

fn handle_ai_progress(body: &[u8]) -> String {
    let v = match parse_body(body) { Ok(v) => v, Err(e) => return e };
    let Some(action) = v["action"].as_str() else {
        return err_resp("bad_request", "action required");
    };
    match ai::progress(action) {
        Some((step, elapsed_ms)) => ok_resp(serde_json::json!({ "progress": { "step": step, "elapsedMs": elapsed_ms } })),
        None => ok_resp(serde_json::json!({ "progress": null })),
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
         Rules: order projects sensibly and group tiles by context: one frame per product or work area. \
         Projects that are parts of the same product share ONE frame titled after the product, e.g. sibling folders under a common parent \
         (game/client, game/server, game/map-editor) or paths naming the same product in different places (a game's mods and its dedicated server). \
         Unrelated projects never share a frame. \
         A frame with a path already represents that project: reuse it via frameId for that project's tiles instead of creating another. \
         Reuse other existing frames via frameId when they fit (omit frameId for new frames). \
         Set path to the projects[].path only when the frame holds a single project; omit it for multi-project frames and frames outside any project. \
         Short titles (2-4 words), never invent tile, frame or project ids.\n\
         Return ONLY the JSON object, no prose.\n\n\
         {snapshot}"
    );

    match tokio::task::spawn_blocking(move || ai::run_oneshot(ai::OneShotOpts { action: "tidy", provider, model, prompt })).await {
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

pub(crate) struct WorkerTile {
    pub run_id: String,
    pub run_title: String,
    pub task_title: String,
    pub status: String,
    pub worktree: Option<String>,
}

pub(crate) fn worker_tiles() -> HashMap<String, WorkerTile> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let mut result: HashMap<String, (u64, WorkerTile)> = HashMap::new();
    for entry in guard.runs.values() {
        for dispatch in entry.dispatches.values() {
            let Some(tile_id) = dispatch.tile_id.as_ref() else { continue };
            let task_title = entry.tasks.get(&dispatch.task_id)
                .map(|t| t.title.clone())
                .unwrap_or_default();
            let status = match dispatch.status {
                DispatchStatus::Running => "running",
                DispatchStatus::Reported => "reported",
                DispatchStatus::Failed => "failed",
                DispatchStatus::Stopped => "stopped",
                DispatchStatus::Stale => "stale",
            };
            let prev_at = result.get(tile_id).map(|(at, _)| *at).unwrap_or(0);
            if dispatch.started_at >= prev_at {
                result.insert(tile_id.clone(), (dispatch.started_at, WorkerTile {
                    run_id: entry.run.id.clone(),
                    run_title: entry.run.title.clone(),
                    task_title,
                    status: status.into(),
                    worktree: dispatch.worktree.clone(),
                }));
            }
        }
    }
    result.into_iter().map(|(k, (_, v))| (k, v)).collect()
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
         - Default to plan and detach: create the run with --detach, dispatch, and end the turn; on wake use check --decisions.\n\
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
    use std::sync::atomic::{AtomicBool, AtomicU32};
    use crate::orch::model::Limits;

    #[test]
    fn transcript_cap_preserves_utf8_tail() {
        const CAP: usize = 2 * 1024 * 1024;
        assert_eq!(cap_worker_transcript("small\ntext"), "small\ntext");
        assert_eq!(cap_worker_transcript(&"a".repeat(CAP)).len(), CAP);
        let text = format!("{}end", "Ã©".repeat(CAP / 2 + 50));
        let capped = cap_worker_transcript(&text);
        let (header, tail) = capped.split_once('\n').unwrap();
        let dropped: usize = header.strip_prefix("[transcript truncated: ").unwrap()
            .strip_suffix(" bytes dropped]").unwrap().parse().unwrap();
        assert!(capped.len() <= CAP);
        assert!(text.is_char_boundary(dropped));
        assert_eq!(tail, &text[dropped..]);
        assert!(tail.ends_with("end"));

        let text = format!("{}\nlast line", "a".repeat(CAP + 10));
        assert!(cap_worker_transcript(&text).ends_with("\nlast line"));
    }

    #[test]
    fn transition_notifications_cover_board_events() {
        let run: Run = serde_json::from_value(serde_json::json!({
            "id": "r", "title": "Board", "repo": "/repo", "status": "active",
            "limits": { "maxWorkers": 1, "maxAttempts": 1, "dispatchTimeoutMin": 1 },
            "createdAt": 1, "updatedAt": 1
        })).unwrap();
        let task: Task = serde_json::from_value(serde_json::json!({
            "id": "t", "title": "Task", "spec": "", "deps": [], "escalation": [], "verify": [],
            "review": false, "status": "pending", "attempts": 1, "createdAt": 1, "updatedAt": 1
        })).unwrap();
        let mut entry = reducer::RunEntry::new(run);
        entry.tasks.insert(task.id.clone(), task);
        let snapshot = || (RunStatus::Active, HashMap::from([("t".to_string(), TaskStatus::Pending)]));
        let data = serde_json::json!({ "taskId": "t", "question": "Proceed?", "files": ["a.rs"] });
        for (kind, expected) in [("gate-create", "gate"), ("task-conflict", "conflict")] {
            let events = transition_notifications(Some(&snapshot()), &entry, kind, &data);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["kind"], expected);
        }
        for (status, expected) in [(TaskStatus::Review, "review"), (TaskStatus::Failed, "exhausted")] {
            entry.tasks.get_mut("t").unwrap().status = status.clone();
            let events = transition_notifications(Some(&snapshot()), &entry, "test", &data);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0]["kind"], expected);
            let unchanged = (RunStatus::Active, HashMap::from([("t".to_string(), status)]));
            assert!(transition_notifications(Some(&unchanged), &entry, "test", &data).is_empty());
        }
        entry.tasks.get_mut("t").unwrap().status = TaskStatus::Pending;
        entry.run.status = RunStatus::Completed;
        let events = transition_notifications(Some(&snapshot()), &entry, "run-set-status", &data);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["kind"], "completed");

        let mut receiver = crate::ui::subscribe_events();
        let mut replayed = State::default();
        replayed.runs.insert("r".into(), entry);
        for (kind, event) in [
            ("gate-create", serde_json::json!({"runId":"r", "id":"g1", "taskId":"t", "question":"Proceed?"})),
            ("task-update", serde_json::json!({"runId":"r", "taskId":"t", "status":"review"})),
            ("task-update", serde_json::json!({"runId":"r", "taskId":"t", "status":"failed"})),
            ("task-conflict", serde_json::json!({"runId":"r", "taskId":"t", "files":["a.rs"]})),
            ("run-set-status", serde_json::json!({"runId":"r", "status":"completed"})),
        ] {
            replayed.replay(kind, &event).unwrap();
        }
        while let Ok(message) = receiver.try_recv() {
            let event: Value = serde_json::from_str(&message).unwrap();
            assert!(event["t"] != "orch.notify" || event["runId"] != "r");
        }
    }

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

    #[tokio::test(flavor = "multi_thread")]
    async fn worker_handoff_is_compact_in_checks_and_full_report_survives_replay() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let parse = |response: String| {
            let value: Value = serde_json::from_str(&response).unwrap();
            assert_eq!(value["ok"], true, "{response}");
            value
        };
        let created = parse(handle_run_create(
            serde_json::json!({"repo": base, "workerSummaryCap": 48}).to_string().as_bytes()
        ).await);
        assert_eq!(created["run"]["workerSummaryCap"], 48);
        let run_id = created["run"]["id"].as_str().unwrap().to_owned();
        let dispatch_id = format!("{run_id}.t1.1");
        parse(handle_run_update(
            serde_json::json!({"runId": run_id, "workerSummaryCap": 32}).to_string().as_bytes()
        ).await);
        let run_cmd = |cmd| {
            assert!(matches!(commit_sync_raw(|_| Ok(cmd), &run_id, &base), CommitResult::Ok(_)));
        };
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t1".into(), title: "Task".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: None, escalation: vec![], verify: vec![],
            role: None, review: false, owner: None, now: 1,
        });
        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), id: dispatch_id.clone(), task_id: "t1".into(), attempt: 1,
            retry_of: None, profile: None, model: None, tile_id: None, merge_branch: None, owner: None, now: 2,
        });
        let blocker = format!("Blocker: disk is read-only {}", "\u{1f680}".repeat(40));
        let summary = format!("Result: {}\n{blocker}", "Long result. ".repeat(10));
        let report = "# Full worker report\r\nEvidence at src/main.rs:42\r\n";
        parse(handle_send(serde_json::json!({
            "runId": run_id, "from": dispatch_id, "to": "coordinator", "type": "worker_done",
            "outcome": "failed", "summary": summary, "report": report, "filesModified": ["src/main.rs"],
        }).to_string().as_bytes()).await);

        let excerpt: String = blocker.chars().take(32).collect();
        let length = summary.chars().count();
        let expected = format!(
            "Outcome: failed\n{excerpt}\n[Truncated; original length: {length} characters]\nChanged files: src/main.rs\nFull report: panorama-cli orch worker-show --run-id {run_id} --dispatch-id {dispatch_id}"
        );
        for decisions in [false, true] {
            let checked = parse(handle_check(serde_json::json!({
                "runId": run_id, "peek": true, "decisions": decisions, "types": ["worker_done"],
            }).to_string().as_bytes()).await);
            assert_eq!(checked["messages"][0]["body"], expected);
        }

        parse(handle_run_update(
            serde_json::json!({"runId": run_id, "detached": true}).to_string().as_bytes()
        ).await);
        let checked = parse(handle_check(serde_json::json!({
            "runId": run_id, "peek": true, "decisions": true,
        }).to_string().as_bytes()).await);
        assert_eq!(checked["messages"][0]["type"], "run_idle");
        let idle = checked["messages"][0]["body"].as_str().unwrap();
        assert!(idle.contains("Blocker: disk is read-only"));

        let late_report = "Late evidence\r\n".repeat(10);
        let seq = state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].next_msg_seq();
        run_cmd(Cmd::Send {
            run_id: run_id.clone(), seq, from: "brain".into(), to: vec!["coordinator".into()],
            msg_type: "late_report".into(), task_id: None, dispatch_id: Some(dispatch_id.clone()),
            reply_to: None, outcome: None, summary: None, files_modified: vec![], now: 3,
            body: Some(serde_json::json!({
                "outcome": "succeeded", "dispatchId": dispatch_id, "report": late_report,
            }).to_string()),
        });
        let checked = parse(handle_check(serde_json::json!({
            "runId": run_id, "peek": true, "decisions": false, "types": ["late_report"],
        }).to_string().as_bytes()).await);
        let notice = checked["messages"][0]["body"].as_str().unwrap();
        assert!(notice.contains("Outcome: succeeded\nLate evidence"));
        assert!(notice.contains("[Truncated; original length: 150 characters]"));
        assert!(!notice.contains(&late_report));

        let mut replayed = State::default();
        load_into(&mut replayed, &run_events_path(&base, &run_id), &run_id);
        let entry = replayed.runs.remove(&run_id).unwrap();
        assert_eq!(entry.run.worker_summary_cap, 32);
        let handoff = entry.messages.iter().find(|m| m.msg_type == "worker_done").unwrap();
        assert_eq!(handoff.body.as_deref(), Some(expected.as_str()));
        state().lock().unwrap_or_else(|e| e.into_inner()).runs.insert(run_id.clone(), entry);
        let show = |full| parse(handle_run_show(
            serde_json::json!({"runId": run_id, "full": full}).to_string().as_bytes()
        ));
        let full = show(true);
        assert_eq!(full["dispatches"][0]["report"], report);
        assert_eq!(full["dispatches"][0]["lateReports"][0]["report"], late_report);
        let standard = show(false);
        assert!(standard["dispatches"][0].get("report").is_none());
        assert!(standard["dispatches"][0].get("lateReports").is_none());
        let worker = parse(handle_worker_show(
            serde_json::json!({"runId": run_id, "dispatchId": dispatch_id}).to_string().as_bytes()
        ));
        assert_eq!(worker["dispatch"]["report"], report);
        assert_eq!(worker["dispatch"]["lateReports"][0]["report"], late_report);
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

    #[test]
    fn detached_finished_digest_is_bounded_and_lists_tasks() {
        let mut entry = mk_run_entry("r1");
        entry.run.detached = true;
        for n in 1..=12 {
            let id = format!("t{n}");
            let mut task = mk_task_entry(&id, TaskStatus::Completed, 1);
            task.integrated_sha = Some("sha".into());
            task.title = "Long title 🚀".repeat(30);
            entry.tasks.insert(id, task);
        }
        let report = autonomy::idle_reason(&entry, unix_ms() as i64).unwrap();
        let digest = idle_digest(&entry, &report);
        assert!(digest.chars().count() <= 3000);
        for n in 1..=12 { assert!(digest.contains(&format!("t{n} ("))); }
        assert!(digest.contains("orch report --run-id r1 --format markdown"));
        assert!(digest.contains("orch run-show --run-id r1 --full"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn detached_worker_question_gets_default_answer() {
        f12_test_home();
        let run_id = format!("question-{}", unix_ms());
        let base = crate::panorama_dir().unwrap();
        replay_task_attempt(&run_id, &base, &[], false);
        let dispatch = format!("{run_id}.t1.1");
        let response = handle_send(serde_json::json!({"runId":run_id, "from":dispatch, "to":"coordinator", "type":"question", "body":"Which approach?"}).to_string().as_bytes()).await;
        assert_eq!(serde_json::from_str::<Value>(&response).unwrap()["ok"], true, "{response}");
        let guard = state().lock().unwrap();
        let entry = &guard.runs[&run_id];
        let question = entry.messages.iter().find(|m| m.msg_type == "question").unwrap();
        assert!(!needs_decision(question, entry, unix_ms() as i64));
        let answer = entry.messages.iter().find(|m| m.msg_type == "answer").unwrap();
        assert_eq!(answer.to, dispatch);
        assert_eq!(answer.reply_to, Some(question.seq));
        assert_eq!(answer.body.as_deref(), Some(DEFAULT_QUESTION_ANSWER));
    }

    #[test]
    fn idle_recovers_only_unanswered_detached_questions() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        for (suffix, detached, preanswered) in [("pending", true, false), ("answered", true, true), ("attached", false, false)] {
            let run_id = format!("question-recovery-{suffix}-{}", unix_ms());
            replay_task_attempt(&run_id, &base, &[], false);
            let dispatch = format!("{run_id}.t1.1");
            let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs.get_mut(&run_id).unwrap().run.detached = detached;
            let question = Cmd::Send {
                run_id: run_id.clone(), seq: guard.runs[&run_id].next_msg_seq(), from: dispatch.clone(),
                to: vec!["coordinator".into()], msg_type: "question".into(), task_id: None,
                dispatch_id: None, reply_to: None, body: Some("Which approach?".into()),
                outcome: None, files_modified: vec![], summary: None, now: unix_ms(),
            };
            assert!(append_and_replay(&mut guard, &run_id, &question, &base).is_ok());
            if preanswered {
                let answer = Cmd::Send {
                    run_id: run_id.clone(), seq: guard.runs[&run_id].next_msg_seq(), from: "coordinator".into(),
                    to: vec![dispatch.clone()], msg_type: "answer".into(), task_id: None, dispatch_id: None,
                    reply_to: None, body: Some("Proceed".into()), outcome: None, files_modified: vec![],
                    summary: None, now: unix_ms(),
                };
                assert!(append_and_replay(&mut guard, &run_id, &answer, &base).is_ok());
            }
            sync_idle_locked(&mut guard, &run_id, &base);
            sync_idle_locked(&mut guard, &run_id, &base);
            let answers: Vec<_> = guard.runs[&run_id].messages.iter().filter(|m| m.msg_type == "answer" && m.to == dispatch).collect();
            assert_eq!(answers.len(), usize::from(detached || preanswered));
            if detached && !preanswered { assert_eq!(answers[0].body.as_deref(), Some(DEFAULT_QUESTION_ANSWER)); }
        }
    }

    #[test]
    fn needs_decision_classifies_detached_messages() {
        let now = 5_000_000_000i64;
        let mut entry = mk_run_entry("r1");
        entry.run.detached = true;
        entry.run.autonomy = Autonomy { operational: true, execution: true, integration: true };
        let mut msg = Message {
            seq: 1, from: String::new(), to: "coordinator".into(), msg_type: String::new(),
            task_id: None, dispatch_id: None, reply_to: None, body: None, at: 0, acked: false,
        };
        for (kind, from, body, expected) in [
            ("integration_conflict", "brain", "", true),
            ("question", "r1.t1.1", "Can I continue?", false),
            ("question", "coordinator", "", false),
            ("question", "brain", "", false),
            ("question", "user", "", false),
            ("status", "r1.t1.1", "Worker waiting for permission", false),
            ("status", "r1.t1.1", "worker waiting on a command", false),
            ("worker_stalled", "brain", "", true),
            ("setup_failed", "brain", "", true),
            ("gate", "brain", "g1", true),
            ("note", "user", "", true),
            ("note", "coordinator", "", false),
            ("late_report", "brain", "", false),
            ("worker_done", "r1.t1.1", r#"{"outcome":"failed"}"#, false),
            ("worker_done", "r1.t1.1", r#"{"outcome":"succeeded"}"#, false),
            ("verification_passed", "brain", "", false),
            ("verification_failed", "brain", "", false),
            ("worker_failed", "brain", "", false),
            ("operational_failure", "brain", "", false),
            ("auto_action", "brain", "", false),
            ("task_integrated", "brain", "", false),
            ("task_waiting", "brain", r#"{"kind":"intervention"}"#, false),
            ("task_waiting", "brain", r#"{"kind":"quota"}"#, false),
            ("resume_ready", "brain", "", false),
            ("task_exhausted", "brain", "", false),
            ("progress", "r1.t1.1", "", false),
        ] {
            msg.msg_type = kind.into();
            msg.from = from.into();
            msg.body = Some(body.into());
            assert_eq!(needs_decision(&msg, &entry, now), expected, "{kind} from {from}: {body}");
            entry.run.detached = false;
            assert!(needs_decision(&msg, &entry, now), "{kind} on an attached run");
            entry.run.detached = true;
        }

        let mut task = mk_task_entry("t1", TaskStatus::Failed, 1);
        task.routing = 0;
        entry.tasks.insert("t1".into(), task);
        let idle_msg = Message {
            seq: 10, from: "brain".into(), to: "coordinator".into(), msg_type: "run_idle".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: Some("{}".into()), at: 0, acked: false,
        };
        entry.messages.push(idle_msg.clone());
        assert!(needs_decision(&idle_msg, &entry, now));

        entry.tasks.get_mut("t1").unwrap().status = TaskStatus::Dispatched;
        assert!(!needs_decision(&idle_msg, &entry, now));

        let t1 = entry.tasks.get_mut("t1").unwrap();
        t1.status = TaskStatus::Completed;
        t1.integrated_sha = Some("abc".into());
        assert!(!needs_decision(&idle_msg, &entry, now));

        let finished_msg = Message {
            seq: 11, from: "brain".into(), to: "coordinator".into(), msg_type: "run_finished".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: Some("{}".into()), at: 0, acked: false,
        };
        entry.messages.push(finished_msg.clone());
        assert!(needs_decision(&finished_msg, &entry, now));

        entry.tasks.get_mut("t1").unwrap().status = TaskStatus::Failed;
        assert!(!needs_decision(&finished_msg, &entry, now));
        entry.tasks.get_mut("t1").unwrap().status = TaskStatus::Dispatched;
        assert!(!needs_decision(&finished_msg, &entry, now));
    }

    #[test]
    fn coordinator_recheck_counts_only_unacked_decisions_when_detached() {
        let now = 5_000_000_000i64;
        let mut entry = mk_run_entry("r1");
        for (msg_type, to, acked) in [
            ("progress", "coordinator", false),
            ("gate", "coordinator", false),
            ("question", "coordinator", true),
            ("integration_conflict", "r1.t1.1", false),
        ] {
            entry.messages.push(Message {
                seq: entry.messages.len() as u32 + 1, from: "r1.t1.1".into(), to: to.into(),
                msg_type: msg_type.into(), task_id: None, dispatch_id: None, reply_to: None,
                body: None, at: 0, acked,
            });
        }
        assert_eq!(coordinator_unacked(&entry, now).count(), 2);
        entry.run.detached = true;
        assert_eq!(coordinator_unacked(&entry, now).count(), 1);
    }

    #[test]
    fn idle_episode_emits_once_and_again_after_leaving_idle_and_replays() {
        let run_id = format!("r-idle-ep-{}", unix_ms());
        let base = std::env::temp_dir().join(format!("panorama-idle-ep-{}", unix_ms()));
        std::fs::create_dir_all(&base).unwrap();
        let run_cmd = |cmd: Cmd| {
            let res = commit_sync(|_| Ok(cmd), &run_id, &base);
            assert!(matches!(res, CommitResult::Ok(_)));
        };

        run_cmd(Cmd::RunCreate {
            id: run_id.clone(), title: "ep".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits { max_attempts: 1, ..Limits::default() }, now: 1,
        }.with_fields(serde_json::json!({
            "detached": true,
            "autonomy": Autonomy { operational: false, execution: true, integration: false },
        })));
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t1".into(), title: "Task 1".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 2,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));
        let count_type = |kind: &str| -> usize {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs[&run_id].messages.iter().filter(|m| m.msg_type == kind).count()
        };
        assert_eq!(count_type("run_idle"), 0);

        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), id: format!("{run_id}.t1.1"), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: Some("codex".into()), model: Some("m".into()),
            tile_id: None, merge_branch: None, owner: None, now: 3,
        });
        run_cmd(Cmd::DispatchUpdate {
            run_id: run_id.clone(), dispatch_id: format!("{run_id}.t1.1"),
            status: DispatchStatus::Failed, summary: Some("boom".into()), now: 4,
        });
        run_cmd(Cmd::TaskRecordFailure {
            run_id: run_id.clone(), task_id: "t1".into(), dispatch_id: format!("{run_id}.t1.1"),
            class: FailureClass::QualityFailed, chain_index: Some(0), now: 5,
        });
        assert_eq!(count_type("run_idle"), 1);

        run_cmd(Cmd::RunUpdate { run_id: run_id.clone(), title: "ep2".into(), now: 6 });
        assert_eq!(count_type("run_idle"), 1);

        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t2".into(), title: "Task 2".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 7,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));
        assert_eq!(state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].idle_state, IdleState::Active);

        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), id: format!("{run_id}.t2.1"), task_id: "t2".into(),
            attempt: 1, retry_of: None, profile: Some("codex".into()), model: Some("m".into()),
            tile_id: None, merge_branch: None, owner: None, now: 8,
        });
        run_cmd(Cmd::DispatchUpdate {
            run_id: run_id.clone(), dispatch_id: format!("{run_id}.t2.1"),
            status: DispatchStatus::Failed, summary: Some("boom again".into()), now: 9,
        });
        run_cmd(Cmd::TaskRecordFailure {
            run_id: run_id.clone(), task_id: "t2".into(), dispatch_id: format!("{run_id}.t2.1"),
            class: FailureClass::QualityFailed, chain_index: Some(0), now: 10,
        });
        assert_eq!(count_type("run_idle"), 2);

        run_cmd(Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: "t1".into(), status: Some(TaskStatus::Cancelled),
            result: None, spec: None, acceptance: None, verify: None, profile: None,
            escalation: None, review: None, owner: None, by: "coordinator".into(), reason: None, now: 11,
        });
        run_cmd(Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: "t2".into(), status: Some(TaskStatus::Cancelled),
            result: None, spec: None, acceptance: None, verify: None, profile: None,
            escalation: None, review: None, owner: None, by: "coordinator".into(), reason: None, now: 12,
        });
        assert_eq!(count_type("run_finished"), 1);

        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t3".into(), title: "Task 3".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 13,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));
        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), id: format!("{run_id}.t3.1"), task_id: "t3".into(),
            attempt: 1, retry_of: None, profile: Some("codex".into()), model: Some("m".into()),
            tile_id: None, merge_branch: None, owner: None, now: 14,
        });
        run_cmd(Cmd::DispatchSetWorktree {
            run_id: run_id.clone(), dispatch_id: format!("{run_id}.t3.1"),
            worktree: "/tmp/wt".into(), branch: "orch/b".into(), now: 15,
        });
        let seq = state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].next_msg_seq();
        run_cmd(Cmd::Send {
            run_id: run_id.clone(), seq, from: format!("{run_id}.t3.1"),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: Some("t3".into()), dispatch_id: Some(format!("{run_id}.t3.1")),
            reply_to: None, body: None, outcome: Some("succeeded".into()),
            files_modified: vec![], summary: Some("done".into()), now: 16,
        });
        let seq = state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].next_msg_seq();
        run_cmd(Cmd::TaskIntegrate {
            run_id: run_id.clone(), task_id: "t3".into(),
            dispatch_id: format!("{run_id}.t3.1"), integrated_sha: "sha1".into(),
            msg_seq: seq, now: 17,
        });
        assert_eq!(count_type("run_finished"), 2);

        let mut replayed = State::default();
        let events_path = run_events_path(&base, &run_id);
        load_into(&mut replayed, &events_path, &run_id);
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let live = &guard.runs[&run_id];
        let fresh = &replayed.runs[&run_id];
        assert_eq!(fresh.idle_state, live.idle_state);
        assert_eq!(fresh.messages.len(), live.messages.len());
        for (a, b) in fresh.messages.iter().zip(live.messages.iter()) {
            assert_eq!((a.seq, &a.msg_type, &a.body), (b.seq, &b.msg_type, &b.body));
        }
        drop(guard);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn autonomy_turned_off_via_run_update_emits_run_idle() {
        let run_id = format!("r-auto-off-{}", unix_ms());
        let base = std::env::temp_dir().join(format!("panorama-auto-off-{}", unix_ms()));
        std::fs::create_dir_all(&base).unwrap();
        let run_cmd = |cmd: Cmd| {
            let res = commit_sync(|_| Ok(cmd), &run_id, &base);
            assert!(matches!(res, CommitResult::Ok(_)));
        };

        run_cmd(Cmd::RunCreate {
            id: run_id.clone(), title: "a".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits::default(), now: 1,
        }.with_fields(serde_json::json!({
            "detached": true,
            "autonomy": Autonomy { operational: false, execution: true, integration: false },
        })));
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t1".into(), title: "Ready".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 2,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));

        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            assert!(guard.runs[&run_id].messages.iter().all(|m| m.msg_type != "run_idle"));
        }

        run_cmd(Cmd::RunUpdate { run_id: run_id.clone(), title: "a".into(), now: 3 }
            .with_fields(serde_json::json!({"autonomy": Autonomy::default()})));

        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let idle_msgs: Vec<&Message> = guard.runs[&run_id].messages.iter()
                .filter(|m| m.msg_type == "run_idle").collect();
            assert_eq!(idle_msgs.len(), 1);
            assert!(idle_msgs[0].body.as_deref().unwrap().contains("t1 (Ready): Pending"));
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn check_decisions_acks_stale_run_idle_as_routine() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let run_id = format!("r-stale-idle-{}", unix_ms());
        let run_cmd = |cmd: Cmd| {
            let res = commit_sync(|_| Ok(cmd), &run_id, &base);
            assert!(matches!(res, CommitResult::Ok(_)));
        };

        run_cmd(Cmd::RunCreate {
            id: run_id.clone(), title: "s".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits::default(), now: 1,
        }.with_fields(serde_json::json!({
            "detached": true,
            "autonomy": Autonomy::default(),
        })));
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t1".into(), title: "Ready".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 2,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));

        let peek_resp = handle_check(
            serde_json::json!({"runId": run_id, "decisions": true, "peek": true}).to_string().as_bytes()
        ).await;
        let peek_val: Value = serde_json::from_str(&peek_resp).unwrap();
        assert_eq!(peek_val["messages"].as_array().unwrap().len(), 1);
        assert_eq!(peek_val["messages"][0]["type"], "run_idle");

        run_cmd(Cmd::RunUpdate { run_id: run_id.clone(), title: "s".into(), now: 3 }
            .with_fields(serde_json::json!({"autonomy": Autonomy { operational: false, execution: true, integration: false }})));

        let resp = handle_check(
            serde_json::json!({"runId": run_id, "decisions": true}).to_string().as_bytes()
        ).await;
        let val: Value = serde_json::from_str(&resp).unwrap();
        assert!(val["messages"].as_array().unwrap().is_empty(), "{resp}");
        assert_eq!(val["routine"]["byType"]["run_idle"], 1, "{resp}");
        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let idle_msg = guard.runs[&run_id].messages.iter().find(|m| m.msg_type == "run_idle").unwrap();
            assert!(idle_msg.acked);
        }

        run_cmd(Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: "t1".into(), status: Some(TaskStatus::Cancelled),
            result: None, spec: None, acceptance: None, verify: None, profile: None,
            escalation: None, review: None, owner: None, by: "coordinator".into(), reason: None, now: 4,
        });
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t2".into(), title: "More".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 5,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));

        let resp2 = handle_check(
            serde_json::json!({"runId": run_id, "decisions": true}).to_string().as_bytes()
        ).await;
        let val2: Value = serde_json::from_str(&resp2).unwrap();
        assert!(val2["messages"].as_array().unwrap().is_empty(), "{resp2}");
        assert_eq!(val2["routine"]["byType"]["run_finished"], 1, "{resp2}");
        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let fin_msg = guard.runs[&run_id].messages.iter().find(|m| m.msg_type == "run_finished").unwrap();
            assert!(fin_msg.acked);
        }
    }

    #[test]
    fn non_detached_run_emits_no_idle_finished_or_exhausted_messages() {
        let now = 5_000_000_000i64;
        let run_id = format!("r-attached-{}", unix_ms());
        let base = std::env::temp_dir().join(format!("panorama-attached-{}", unix_ms()));
        std::fs::create_dir_all(&base).unwrap();
        let run_cmd = |cmd: Cmd| {
            let res = commit_sync(|_| Ok(cmd), &run_id, &base);
            assert!(matches!(res, CommitResult::Ok(_)));
        };

        run_cmd(Cmd::RunCreate {
            id: run_id.clone(), title: "att".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits { max_attempts: 1, ..Limits::default() }, now: 1,
        });
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t1".into(), title: "T1".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 2,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));
        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), id: format!("{run_id}.t1.1"), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: Some("codex".into()), model: Some("m".into()),
            tile_id: None, merge_branch: None, owner: None, now: 3,
        });
        run_cmd(Cmd::DispatchUpdate {
            run_id: run_id.clone(), dispatch_id: format!("{run_id}.t1.1"),
            status: DispatchStatus::Failed, summary: Some("err".into()), now: 4,
        });
        run_cmd(Cmd::TaskRecordFailure {
            run_id: run_id.clone(), task_id: "t1".into(), dispatch_id: format!("{run_id}.t1.1"),
            class: FailureClass::QualityFailed, chain_index: Some(0), now: 5,
        });
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t2".into(), title: "T2".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()),
            escalation: vec!["codex:m".into()], verify: vec![],
            role: None, review: false, owner: None, now: 6,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));
        let seq = state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].next_msg_seq();
        run_cmd(Cmd::GateCreate {
            run_id: run_id.clone(), id: "g1".into(), task_id: "t2".into(),
            question: "ok?".into(), options: vec![], msg_seq: seq, now: 7,
        });
        run_cmd(Cmd::GateResolve {
            run_id: run_id.clone(), gate_id: "g1".into(), resolution: "yes".into(), now: 8,
        });
        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), id: format!("{run_id}.t2.1"), task_id: "t2".into(),
            attempt: 1, retry_of: None, profile: Some("codex".into()), model: Some("m".into()),
            tile_id: None, merge_branch: None, owner: None, now: 9,
        });
        run_cmd(Cmd::DispatchSetWorktree {
            run_id: run_id.clone(), dispatch_id: format!("{run_id}.t2.1"),
            worktree: "/tmp/wt".into(), branch: "orch/b".into(), now: 10,
        });
        let seq = state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].next_msg_seq();
        run_cmd(Cmd::Send {
            run_id: run_id.clone(), seq, from: format!("{run_id}.t2.1"),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: Some("t2".into()), dispatch_id: Some(format!("{run_id}.t2.1")),
            reply_to: None, body: None, outcome: Some("succeeded".into()),
            files_modified: vec![], summary: Some("done".into()), now: 11,
        });
        let seq = state().lock().unwrap_or_else(|e| e.into_inner()).runs[&run_id].next_msg_seq();
        run_cmd(Cmd::TaskIntegrate {
            run_id: run_id.clone(), task_id: "t2".into(),
            dispatch_id: format!("{run_id}.t2.1"), integrated_sha: "sha1".into(),
            msg_seq: seq, now: 12,
        });
        run_cmd(Cmd::TaskUpdate {
            run_id: run_id.clone(), task_id: "t1".into(), status: Some(TaskStatus::Cancelled),
            result: None, spec: None, acceptance: None, verify: None, profile: None,
            escalation: None, review: None, owner: None, by: "coordinator".into(), reason: None, now: 13,
        });
        run_cmd(Cmd::RunUpdate { run_id: run_id.clone(), title: "att2".into(), now: 14 });

        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = &guard.runs[&run_id];
        for forbidden in ["run_idle", "run_finished", "task_exhausted"] {
            assert!(
                entry.messages.iter().all(|m| m.msg_type != forbidden),
                "unexpected {forbidden} message on attached run"
            );
        }
        assert!(entry.messages.iter().filter(|m| m.to == "coordinator").all(|m| needs_decision(m, entry, now)));
        drop(guard);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn check_decisions_acks_and_summarizes_routine_regardless_of_type() {
        f12_test_home();
        let run_id = "r-check-decisions-digest";
        setup_run_in_state(run_id);
        {
            let mut guard = state().lock().unwrap();
            guard.replay("run-update", &serde_json::json!({"runId": run_id, "detached": true})).unwrap();
            guard.replay("task-create", &serde_json::json!({
                "runId": run_id, "id": "t1", "title": "Write code",
            })).unwrap();
            let entry = guard.runs.get_mut(run_id).unwrap();
            entry.run.autonomy.integration = true;
            for (msg_type, to) in [
                ("progress", "coordinator"),
                ("verification_passed", "coordinator"),
                ("gate", "coordinator"),
                ("integration_conflict", "coordinator"),
                ("progress", "r1.t1.1"),
            ] {
                entry.msg_seq = entry.next_msg_seq();
                entry.messages.push(Message {
                    seq: entry.msg_seq, from: "r1.t1.1".into(), to: to.into(),
                    msg_type: msg_type.into(), task_id: Some("t1".into()), dispatch_id: None,
                    reply_to: None, body: Some(format!("{msg_type} full body")), at: 0, acked: false,
                });
            }
        }
        let body = serde_json::json!({"runId": run_id, "decisions": true, "types": ["gate"]});
        let response = handle_check(body.to_string().as_bytes()).await;
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["ok"], true, "{response}");
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
        assert_eq!(value["messages"][0]["type"], "gate");
        assert_eq!(value["messages"][0]["body"], "gate full body");
        assert_eq!(value["routine"], serde_json::json!({
            "count": 2,
            "byType": {"progress": 1, "verification_passed": 1},
            "tasks": [{"id": "t1", "title": "Write code", "status": "pending"}],
        }));
        let guard = state().lock().unwrap();
        let entry = &guard.runs[run_id];
        assert_eq!(entry.messages.iter().map(|m| m.acked).collect::<Vec<_>>(),
            [true, true, true, false, false]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn check_decisions_waits_past_routine_messages() {
        f12_test_home();
        let run_id = "r-check-decisions-wait";
        setup_run_in_state(run_id);
        state().lock().unwrap().runs.get_mut(run_id).unwrap().run.detached = true;
        inject_message(run_id, "coordinator");
        let body = serde_json::json!({
            "runId": run_id, "decisions": true, "wait": true, "timeoutMs": 5000,
        }).to_string();
        let check = tokio::spawn(async move { handle_check(body.as_bytes()).await });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!check.is_finished());
        inject_message(run_id, "coordinator");
        state().lock().unwrap().runs.get_mut(run_id).unwrap()
            .messages.last_mut().unwrap().from = "user".into();
        run_notify(run_id).notify_waiters();
        let response = tokio::time::timeout(Duration::from_secs(3), check).await.unwrap().unwrap();
        let value: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(value["ok"], true, "{response}");
        assert_eq!(value["messages"].as_array().unwrap().len(), 1);
        assert_eq!(value["messages"][0]["from"], "user");
        assert_eq!(value["routine"]["byType"], serde_json::json!({"note": 1}));
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

    pub(super) fn mk_run_entry(run_id: &str) -> crate::orch::reducer::RunEntry {
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
            autonomy: Autonomy::default(),
            detached: false, corrective_retry: true, worker_summary_cap: default_worker_summary_cap(),
            coordinator_wait: None,
            created_at: 1_000_000,
            updated_at: 1_000_000,
        };
        RunEntry::new(run)
    }

    pub(super) fn mk_task_entry(id: &str, status: TaskStatus, attempts: u32) -> Task {
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
            owner_reason: None,
            status,
            result: None,
            integrated_sha: None,
            attempts,
            routing: 0, quality_floor: 0, quality_attempts: 0, operational_attempts: 0, pinned: false, last_failure: None, wait: None,
            created_at: 1_000_000,
            updated_at: 1_000_000,
        }
    }

    pub(super) fn mk_dispatch_entry(id: &str, task_id: &str, attempt: u32, retry_of: Option<&str>, cost_usd: Option<f64>, cost_source: Option<&str>, duration_ms: u64) -> Dispatch {
        Dispatch {
            id: id.to_string(),
            task_id: task_id.to_string(),
            attempt,
            retry_of: retry_of.map(str::to_string),
            profile: Some("claude".to_string()),
            model: None,
            effort: None,
            tile_id: None,
            merge_branch: None,
            worktree: None,
            branch: None,
            status: DispatchStatus::Reported,
            outcome: None,
            head_sha: None,
            files_modified: vec![],
            summary: None,
            report: None,
            started_at: 1_000_000,
            ended_at: Some(1_000_000 + duration_ms),
            last_heartbeat: None,
            verifications: vec![],
            cost_usd,
            cost_source: cost_source.map(str::to_string),
            account: None, chain_index: None, trigger: None, decision: None, failure_class: None, by: None, start_ref: None,
            native_home: None, native_session: None, corrective_retry: false, retry_same_entry: false, operational_wait: false,
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

    fn watch_at(now_secs: u64) -> DispatchWatch {
        DispatchWatch {
            consecutive_shell_ticks: 0,
            ever_live: true,
            last_screen: String::new(),
            screen_last_changed_secs: now_secs,
            sent: std::collections::HashSet::new(),
        }
    }

    fn quiet_facts(watch: &DispatchWatch, now_secs: u64) -> TickFacts {
        let mut facts = base_facts();
        facts.agent_status = None;
        facts.screen_quiet_secs = now_secs.saturating_sub(watch.screen_last_changed_secs);
        facts
    }

    #[test]
    fn screen_change_rearms_quiet_so_a_new_episode_is_assessed_again() {
        let quiet_secs = 5 * 60 + 1;
        let mut watch = watch_at(0);
        watch.observe_screen("working", 0);
        let first = assess(&quiet_facts(&watch, quiet_secs), &watch.sent);
        assert!(first.contains(&Finding::Quiet));
        watch.sent.insert("quiet".into());

        watch.observe_screen("working", quiet_secs + 20);
        assert!(!assess(&quiet_facts(&watch, quiet_secs + 20), &watch.sent).contains(&Finding::Quiet));

        watch.observe_screen("usage limit reached", quiet_secs + 40);
        assert!(!watch.sent.contains("quiet"));
        assert!(assess(&quiet_facts(&watch, quiet_secs + 40), &watch.sent).is_empty());
        let later = 2 * quiet_secs + 40;
        assert!(assess(&quiet_facts(&watch, later), &watch.sent).contains(&Finding::Quiet));
    }

    #[test]
    fn screen_change_keeps_other_sent_reasons() {
        let mut watch = watch_at(0);
        watch.sent.insert("quiet".into());
        watch.sent.insert("timeout".into());
        watch.observe_screen("new text", 10);
        assert!(!watch.sent.contains("quiet"));
        assert!(watch.sent.contains("timeout"));
    }

    #[test]
    fn unchanged_screen_keeps_quiet_sent() {
        let mut watch = watch_at(0);
        watch.observe_screen("same", 0);
        watch.sent.insert("quiet".into());
        watch.observe_screen("same", 400);
        assert!(watch.sent.contains("quiet"));
        assert!(!assess(&quiet_facts(&watch, 400), &watch.sent).contains(&Finding::Quiet));
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

        let report = build_report(&entry, None);
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

        let report = build_report(&entry, None);
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

        let report = build_report(&entry, None);
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

        let report = build_report(&entry, None);
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

        let report = build_report(&entry, None);
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

        let report = build_report(&entry, None);
        let md = render_report_markdown(&report, &usage::RunUsage::snapshot(&entry).read());
        assert!(md.contains("## Usage\n\n- Coordinator wakes: 0 (event log)"));
        assert!(md.contains("Coordinator tokens: input unavailable"));
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
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: 1_000_000,
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: 1_002_000,
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: 1_001_000,
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: 1_001_000,
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
            assert!(ctx.contains("Default to plan and detach: create the run with --detach, dispatch, and end the turn; on wake use check --decisions."));
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
                owner: None, merge_branch: None, now: unix_ms(),
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
            limits: Limits::default(), autonomy: Autonomy::default(), detached: false, corrective_retry: true,
            worker_summary_cap: default_worker_summary_cap(), coordinator_wait: None,
            created_at: 0, updated_at: 0,
        };
        let mut entry = RunEntry::new(run);
        let t1 = Task {
            id: "t1".into(), title: "t1".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec![], verify: vec![],
            role: Some(Role::Explore), review: false, owner: None, owner_reason: None,
            status: TaskStatus::Completed, result: None, integrated_sha: None,
            attempts: 1, routing: 0, quality_floor: 0, quality_attempts: 0, operational_attempts: 0, pinned: false, last_failure: None, wait: None, created_at: 0, updated_at: 0,
        };
        let mut t2 = t1.clone();
        t2.id = "t2".into();
        t2.role = Some(Role::Implement);
        entry.tasks.insert("t1".into(), t1);
        entry.tasks.insert("t2".into(), t2);
        let d1 = Dispatch {
            id: "d1".into(), task_id: "t1".into(), attempt: 1, retry_of: None,
            profile: Some("claude".into()), model: Some("haiku".into()),
            effort: None,
            tile_id: None, merge_branch: None, worktree: None, branch: None, status: DispatchStatus::Reported,
            outcome: None, head_sha: None, files_modified: vec![], summary: None, report: None,
            started_at: 0, ended_at: None, last_heartbeat: None, verifications: vec![],
            cost_usd: Some(0.1), cost_source: Some("exact".into()),
            account: None, chain_index: None, trigger: None, decision: None, failure_class: None, by: None, start_ref: None,
            native_home: None, native_session: None, corrective_retry: false, retry_same_entry: false, operational_wait: false,
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

    #[test]
    fn autonomy_flag_parses_comma_list_and_empty_clears() {
        let v = serde_json::json!({"autonomy": "operational, integration"});
        let a = parse_autonomy_field(&v).unwrap().unwrap();
        assert_eq!(a, Autonomy { operational: true, execution: false, integration: true });
        let cleared = parse_autonomy_field(&serde_json::json!({"autonomy": ""})).unwrap().unwrap();
        assert_eq!(cleared, Autonomy::default());
        assert!(parse_autonomy_field(&serde_json::json!({})).unwrap().is_none());
        assert!(parse_autonomy_field(&serde_json::json!({"autonomy": "nope"})).is_err());
    }

    #[test]
    fn parse_limits_defaults_and_reads_max_operational_attempts() {
        let l = parse_limits(&serde_json::json!({})).unwrap();
        assert_eq!(l.max_operational_attempts, 6);
        let l = parse_limits(&serde_json::json!({"maxOperationalAttempts": 3})).unwrap();
        assert_eq!(l.max_operational_attempts, 3);
    }

    const F12_NOW: i64 = 1_000_000_000;
    const F12_HOUR: i64 = 3_600_000;

    struct F12Task<'a> {
        escalation: &'a [&'a str],
        profile: Option<&'a str>,
        pinned: bool,
        routing: u32,
    }

    pub(super) fn f12_commit(s: &mut State, cmd: Cmd) -> (String, Value) {
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) | ApplyResult::Stale(k, d) => {
                s.replay(&k, &d).expect("replay");
                (k, d)
            }
            ApplyResult::Err(e) => panic!("command rejected: {}", e.message()),
        }
    }

    fn f12_state(task: F12Task) -> State {
        let mut s = State::default();
        f12_commit(&mut s, Cmd::RunCreate {
            id: "r1".into(), title: "t".into(), repo: "/tmp".into(), base_ref: None, base_sha: None,
            setup: None, coordinator_tile: None, limits: Limits::default(), now: 1_000_000,
        });
        f12_commit(&mut s, Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "t".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: task.profile.map(str::to_string),
            escalation: task.escalation.iter().map(|e| e.to_string()).collect(),
            verify: vec![], role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(serde_json::json!({"routing": task.routing, "pinned": task.pinned})));
        s
    }

    fn f12_plan(s: &State, states: &candidates::CandidateStates, explicit_profile: Option<&str>) -> wiring::DispatchPlan {
        wiring::plan_dispatch(wiring::PlanInput {
            entry: &s.runs["r1"], task_id: "t1", explicit_profile, explicit_model: None,
            by: DispatchBy::Coordinator, resumed: false, now: F12_NOW,
        }, states).expect("plan")
    }

    fn f12_dispatch(s: &mut State, states: &candidates::CandidateStates) -> (String, wiring::PlannedDispatch) {
        let wiring::DispatchPlan::Dispatch(planned) = f12_plan(s, states, None) else { panic!("expected dispatch") };
        let attempt = s.runs["r1"].tasks["t1"].attempts + 1;
        let id = format!("r1.t1.{attempt}");
        f12_commit(s, Cmd::Dispatch { merge_branch: None,
            run_id: "r1".into(), id: id.clone(), task_id: "t1".into(), attempt, retry_of: None,
            profile: Some(planned.profile.clone()), model: planned.model.clone(), tile_id: None, owner: None, now: 1_000_000,
        }.with_fields(wiring::dispatch_fields(&planned, Some("system".into()))));
        (id, planned)
    }

    fn f12_classification(class: FailureClass, reset_at: Option<i64>) -> failure::Classification {
        failure::Classification {
            class, scope: failure::Scope::Model, reset_at,
            source: "test".into(), confidence: failure::Confidence::Reliable,
        }
    }

    fn f12_fail(s: &mut State, states: &mut candidates::CandidateStates, dispatch_id: &str, found: failure::Classification) -> wiring::AfterFailure {
        f12_commit(s, Cmd::DispatchUpdate {
            run_id: "r1".into(), dispatch_id: dispatch_id.into(), status: DispatchStatus::Failed, summary: None, now: 1_000_000,
        });
        let chain_index = s.runs["r1"].dispatches[dispatch_id].chain_index;
        f12_commit(s, Cmd::TaskRecordFailure {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: dispatch_id.into(),
            class: found.class, chain_index, now: 1_000_000,
        });
        wiring::mark_candidate(states, &s.runs["r1"].dispatches[dispatch_id], &found, F12_NOW);
        wiring::after_failure(wiring::AfterFailureInput {
            entry: &s.runs["r1"], states, dispatch_id, live: false, now: F12_NOW,
        })
    }

    fn f12_expect_dispatch(plan: wiring::DispatchPlan) -> wiring::PlannedDispatch {
        match plan {
            wiring::DispatchPlan::Dispatch(p) => p,
            wiring::DispatchPlan::Wait(w) => panic!("expected dispatch, got wait: {}", w.reason),
        }
    }

    fn f12_expect_wait(plan: wiring::DispatchPlan) -> WaitRequest {
        match plan {
            wiring::DispatchPlan::Wait(w) => w,
            wiring::DispatchPlan::Dispatch(p) => panic!("expected wait, got dispatch of {}", p.profile),
        }
    }

    const F12_CHAIN: &[&str] = &["claude:opus", "codex:gpt", "kimi"];

    #[test]
    fn f12_operational_failure_falls_back_without_spending_quality() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        let mut states = candidates::CandidateStates::default();
        let (d1, first) = f12_dispatch(&mut s, &states);
        assert_eq!((first.profile.as_str(), first.trigger, first.chain_index), ("claude", Trigger::Initial, Some(0)));
        let after = f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::QuotaExhausted, None));
        assert_eq!(after.suggested.map(|c| c.agent), Some("codex".to_string()));
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!((t.quality_attempts, t.quality_floor, t.operational_attempts), (0, 0, 1));
        let next = f12_expect_dispatch(f12_plan(&s, &states, None));
        assert_eq!((next.profile.as_str(), next.model.as_deref()), ("codex", Some("gpt")));
        assert_eq!((next.trigger, next.chain_index), (Trigger::Fallback, Some(1)));
        assert_eq!(next.decision.requested.map(|c| c.agent), Some("claude".to_string()));
        assert!(next.decision.reason.starts_with("fallback:"));
    }

    #[test]
    fn f12_quality_failure_moves_floor_and_escalates() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        let mut states = candidates::CandidateStates::default();
        let (d1, _) = f12_dispatch(&mut s, &states);
        f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::QualityFailed, None));
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!((t.quality_attempts, t.quality_floor, t.operational_attempts), (1, 1, 0));
        let next = f12_expect_dispatch(f12_plan(&s, &states, None));
        assert_eq!((next.profile.as_str(), next.trigger, next.chain_index), ("codex", Trigger::Escalation, Some(1)));
    }

    #[test]
    fn f12_pinned_retry_with_blocked_candidate_waits_instead_of_substituting() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: Some("claude:opus"), pinned: true, routing: 1 });
        let mut states = candidates::CandidateStates::default();
        let (d1, first) = f12_dispatch(&mut s, &states);
        assert_eq!((first.profile.as_str(), first.chain_index), ("claude", None));
        let after = f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::QuotaExhausted, Some(F12_NOW + 5 * F12_HOUR)));
        let wait = after.wait.expect("pinned task waits");
        assert_eq!(wait.kind, WaitKind::Quota);
        assert_eq!(wait.resume_at, Some(F12_NOW + 5 * F12_HOUR));
        assert_eq!(wait.candidate.map(|c| c.agent), Some("claude".to_string()));
        let again = f12_expect_wait(f12_plan(&s, &states, None));
        assert!(again.reason.starts_with("pinned:"));
    }

    #[test]
    fn f12_all_candidates_blocked_waits_until_earliest_known_reset() {
        let mut s = f12_state(F12Task { escalation: &["claude:opus", "codex:gpt"], profile: None, pinned: false, routing: 1 });
        let mut states = candidates::CandidateStates::default();
        let (d1, _) = f12_dispatch(&mut s, &states);
        f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::QuotaExhausted, Some(F12_NOW + 8 * F12_HOUR)));
        let (d2, _) = f12_dispatch(&mut s, &states);
        let after = f12_fail(&mut s, &mut states, &d2, f12_classification(FailureClass::QuotaExhausted, Some(F12_NOW + 3 * F12_HOUR)));
        let wait = after.wait.expect("all blocked waits");
        assert_eq!((wait.kind, wait.resume_at), (WaitKind::Quota, Some(F12_NOW + 3 * F12_HOUR)));
        assert!(!wait.same_session);
        f12_commit(&mut s, Cmd::TaskWait { run_id: "r1".into(), task_id: "t1".into(), wait, now: 2_000_000 });
        let task = &s.runs["r1"].tasks["t1"];
        assert_eq!(task.status, TaskStatus::Waiting);
        assert_eq!(task.wait.as_ref().and_then(|w| w.resume_at), Some(F12_NOW + 3 * F12_HOUR));
    }

    #[test]
    fn f12_operational_cap_waits_for_intervention() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        s.runs.get_mut("r1").unwrap().run.limits.max_operational_attempts = 1;
        let mut states = candidates::CandidateStates::default();
        let (d1, _) = f12_dispatch(&mut s, &states);
        let after = f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::ExecutionFailed, None));
        let wait = after.wait.expect("cap reached");
        assert_eq!((wait.kind, wait.reason.as_str()), (WaitKind::Intervention, "operational attempts exhausted"));
        let entry = &s.runs["r1"];
        assert!(matches!(wiring::retry_limit(entry, &entry.tasks["t1"]), wiring::RetryLimit::OperationalExhausted));
    }

    #[test]
    fn f12_quality_cap_rejects_retry_and_keeps_task_failed() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        s.runs.get_mut("r1").unwrap().run.limits.max_attempts = 1;
        let mut states = candidates::CandidateStates::default();
        let (d1, _) = f12_dispatch(&mut s, &states);
        let after = f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::QualityFailed, None));
        assert!(after.wait.is_none() && after.suggested.is_none());
        let entry = &s.runs["r1"];
        assert!(matches!(wiring::retry_limit(entry, &entry.tasks["t1"]), wiring::RetryLimit::QualityExhausted));
        assert_eq!(entry.tasks["t1"].status, TaskStatus::Failed);
    }

    #[test]
    fn f12_setup_failure_waits_for_intervention_without_counting() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        let mut states = candidates::CandidateStates::default();
        let (d1, _) = f12_dispatch(&mut s, &states);
        let after = f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::SetupFailed, None));
        assert_eq!(after.wait.map(|w| w.kind), Some(WaitKind::Intervention));
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!((t.quality_attempts, t.operational_attempts), (0, 0));
    }

    #[test]
    fn f12_legacy_task_keeps_attempt_indexed_escalation() {
        let s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 0 });
        let task = &s.runs["r1"].tasks["t1"];
        assert_eq!(task.routing, 0);
        for (attempt, expected) in [(1, "claude"), (2, "codex"), (3, "kimi"), (4, "kimi")] {
            let (profile, _) = pick_escalation(EscalationInput {
                escalation: &task.escalation, attempt, explicit_profile: None, explicit_model: None,
                task_profile: None, latest_profile: None, latest_model: None,
            });
            assert_eq!(profile, expected);
        }
        let after = wiring::after_failure(wiring::AfterFailureInput {
            entry: &s.runs["r1"], states: &candidates::CandidateStates::default(), dispatch_id: "none", live: false, now: F12_NOW,
        });
        assert!(after.wait.is_none() && after.suggested.is_none());
    }

    #[test]
    fn f12_legacy_failure_stores_only_failure_class() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 0 });
        f12_commit(&mut s, Cmd::Dispatch { merge_branch: None,
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(), attempt: 1, retry_of: None,
            profile: Some("claude".into()), model: None, tile_id: None, owner: None, now: 1_000_000,
        });
        let failure = DispatchFailure { classification: f12_classification(FailureClass::QuotaExhausted, None), routing: 0 };
        let cmd = with_failure_class(Cmd::DispatchUpdate {
            run_id: "r1".into(), dispatch_id: "r1.t1.1".into(), status: DispatchStatus::Failed, summary: None, now: 1_000_000,
        }, Some(&failure));
        f12_commit(&mut s, cmd);
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!(s.runs["r1"].dispatches["r1.t1.1"].failure_class, Some(FailureClass::QuotaExhausted));
        assert_eq!((t.quality_attempts, t.operational_attempts, t.last_failure, t.status.clone()), (0, 0, None, TaskStatus::Failed));
    }

    #[test]
    fn f12_notices_are_deterministic_and_not_repeated() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        let mut states = candidates::CandidateStates::default();
        let (d1, _) = f12_dispatch(&mut s, &states);
        let after = f12_fail(&mut s, &mut states, &d1, f12_classification(FailureClass::RateLimited, None));
        let suggested = after.suggested.expect("fallback available");
        let body = wiring::operational_failure_body(FailureClass::RateLimited, &d1, &suggested);
        let parsed: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            (parsed["class"].as_str(), parsed["suggested"]["agent"].as_str(), parsed["hint"].as_str()),
            (Some("rate_limited"), Some("codex"), Some("run retry"))
        );
        let notice = |s: &State| wiring::notice_cmd(&s.runs["r1"], wiring::NoticeInput {
            run_id: "r1", task_id: "t1", dispatch_id: Some(&d1), msg_type: "operational_failure", body: body.clone(), now: 2_000_000,
        });
        let first = notice(&s).expect("first notice");
        f12_commit(&mut s, first);
        assert!(notice(&s).is_none());
    }

    #[test]
    fn f12_waiting_tasks_and_decisions_survive_replay() {
        let mut s = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        let mut log: Vec<(String, Value)> = Vec::new();
        let states = candidates::CandidateStates::default();
        let wiring::DispatchPlan::Dispatch(planned) = f12_plan(&s, &states, None) else { panic!("dispatch") };
        let cmds = vec![
            Cmd::Dispatch { merge_branch: None,
                run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(), attempt: 1, retry_of: None,
                profile: Some(planned.profile.clone()), model: planned.model.clone(), tile_id: None, owner: None, now: 1_000_000,
            }.with_fields(wiring::dispatch_fields(&planned, Some("system".into()))),
            Cmd::DispatchUpdate {
                run_id: "r1".into(), dispatch_id: "r1.t1.1".into(), status: DispatchStatus::Failed, summary: None, now: 1_000_001,
            },
            Cmd::TaskRecordFailure {
                run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
                class: FailureClass::QuotaExhausted, chain_index: Some(0), now: 1_000_002,
            },
            Cmd::TaskWait { run_id: "r1".into(), task_id: "t1".into(), wait: wiring::operational_cap_wait(), now: 1_000_003 },
        ];
        for cmd in cmds {
            log.push(f12_commit(&mut s, cmd));
        }

        let mut fresh = f12_state(F12Task { escalation: F12_CHAIN, profile: None, pinned: false, routing: 1 });
        for (kind, data) in &log {
            fresh.replay(kind, data).expect("replay");
        }
        let task = &fresh.runs["r1"].tasks["t1"];
        assert_eq!(task.status, TaskStatus::Waiting);
        assert_eq!(task.wait.as_ref().map(|w| w.kind), Some(WaitKind::Intervention));
        assert_eq!(task.wait, s.runs["r1"].tasks["t1"].wait);
        let d = &fresh.runs["r1"].dispatches["r1.t1.1"];
        assert_eq!((d.trigger, d.chain_index, d.by, d.account.as_deref()), (Some(Trigger::Initial), Some(0), Some(DispatchBy::Coordinator), Some("system")));
        assert_eq!(d.decision.as_ref().map(|x| x.reason.as_str()), Some("preferred model available"));
        assert_eq!(d.failure_class, Some(FailureClass::QuotaExhausted));
    }

    pub(super) fn f12_test_home() {
        static HOME: std::sync::Once = std::sync::Once::new();
        HOME.call_once(|| {
            let dir = std::env::temp_dir().join(format!("panorama-f12-{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("temp home");
            std::env::set_var("PANORAMA_CONFIG_DIR", dir);
        });
    }

    fn f12_global_run(run_id: &str, profile: &str, model: Option<&str>) {
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let mut s = State::default();
        f12_commit(&mut s, Cmd::RunCreate {
            id: run_id.into(), title: "t".into(), repo: "/tmp".into(), base_ref: None, base_sha: None,
            setup: None, coordinator_tile: None, limits: Limits::default(), now: 1_000_000,
        });
        f12_commit(&mut s, Cmd::TaskCreate {
            run_id: run_id.into(), id: "t1".into(), title: "t".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec![format!("{profile}:{}", model.unwrap_or("m"))],
            verify: vec![], role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false})));
        f12_commit(&mut s, Cmd::Dispatch { merge_branch: None,
            run_id: run_id.into(), id: format!("{run_id}.t1.1"), task_id: "t1".into(), attempt: 1, retry_of: None,
            profile: Some(profile.into()), model: model.map(str::to_string), tile_id: None, owner: None, now: 1_000_000,
        });
        guard.runs.extend(s.runs);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f12_live_quota_tail_fails_dispatch_without_worker_stalled() {
        f12_test_home();
        let run_id = format!("r-f12-live-{}", unix_ms());
        f12_global_run(&run_id, "claude", None);
        let dispatch_id = format!("{run_id}.t1.1");
        let screen = "working...\nYou've hit your session limit \u{b7} resets 3am (America/Sao_Paulo)\n";

        assert!(fail_live_dispatch_on_quota(&run_id, &dispatch_id, screen).await);

        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = &guard.runs[&run_id];
        let d = &entry.dispatches[&dispatch_id];
        assert_eq!((d.status.clone(), d.failure_class), (DispatchStatus::Failed, Some(FailureClass::QuotaExhausted)));
        assert!(entry.messages.iter().all(|m| m.msg_type != "worker_stalled"));
        let t = &entry.tasks["t1"];
        assert_eq!((t.operational_attempts, t.quality_attempts, t.last_failure), (1, 0, Some(FailureClass::QuotaExhausted)));
        drop(guard);
        candidate_states().lock().unwrap_or_else(|e| e.into_inner())
            .clear(&candidates::CandidateKey::from_chain_entry("claude"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f12_live_screen_without_quota_text_is_left_alone() {
        f12_test_home();
        let run_id = format!("r-f12-quiet-{}", unix_ms());
        f12_global_run(&run_id, "claude", None);
        let dispatch_id = format!("{run_id}.t1.1");
        assert!(!fail_live_dispatch_on_quota(&run_id, &dispatch_id, "compiling...\ndone\n").await);
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.runs[&run_id].dispatches[&dispatch_id].status, DispatchStatus::Running);
    }

    #[test]
    fn f12_success_clears_the_candidate() {
        f12_test_home();
        let run_id = format!("r-f12-ok-{}", unix_ms());
        f12_global_run(&run_id, "codex", Some("f12-success-model"));
        let key = candidates::CandidateKey::from_chain_entry("codex:f12-success-model");
        {
            let mut states = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
            assert!(states.mark(candidates::MarkInput {
                key: &key, class: FailureClass::RateLimited, reset_at: Some(unix_ms() as i64 + F12_HOUR),
                source: "test", confidence: candidates::Confidence::Reliable, now: unix_ms() as i64,
            }));
            assert!(!states.lookup(&key, unix_ms() as i64).state.is_eligible());
        }
        clear_candidate_on_success(&run_id, &format!("{run_id}.t1.1"));
        let states = candidate_states().lock().unwrap_or_else(|e| e.into_inner());
        assert!(states.lookup(&key, unix_ms() as i64).state.is_eligible());
    }

    fn f12_chain_dispatch(run_id: &str, tile: Option<&str>) -> String {
        let next_id = format!("{run_id}.t1.2");
        let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.runs.get_mut(run_id).expect("run");
        let prev = entry.dispatches.get_mut(&format!("{run_id}.t1.1")).expect("previous dispatch");
        prev.status = DispatchStatus::Failed;
        prev.tile_id = tile.map(str::to_string);
        prev.worktree = Some("/nonexistent/f12-worktree".into());
        prev.branch = Some("orch/f12/t1-1".into());
        let mut next = prev.clone();
        next.id = next_id.clone();
        next.attempt = 2;
        next.status = DispatchStatus::Running;
        next.tile_id = None;
        next.worktree = None;
        next.branch = None;
        next.trigger = Some(Trigger::Fallback);
        entry.dispatches.insert(next_id.clone(), next);
        next_id
    }

    #[tokio::test]
    async fn f12_carry_refuses_while_previous_worker_is_still_live() {
        f12_test_home();
        let run_id = format!("r-f12-carry-live-{}", unix_ms());
        f12_global_run(&run_id, "claude", None);
        let next_id = f12_chain_dispatch(&run_id, Some("tile-still-live"));
        let kills = AtomicU32::new(0);

        let result = prepare_carry_with(&run_id, &next_id, "base".into(), |tile| {
            let kills = &kills;
            async move { stop_worker_with(&tile, |_| true, |_| { kills.fetch_add(1, Ordering::SeqCst); }).await }
        }).await;

        assert_eq!(result.err().as_deref(), Some("previous worker still live"));
        assert_eq!(kills.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn f12_carry_stops_previous_worker_before_proceeding() {
        f12_test_home();
        let run_id = format!("r-f12-carry-stop-{}", unix_ms());
        f12_global_run(&run_id, "claude", None);
        let next_id = f12_chain_dispatch(&run_id, Some("tile-dies-on-kill"));
        let live = AtomicBool::new(true);
        let kills = AtomicU32::new(0);

        let result = prepare_carry_with(&run_id, &next_id, "base".into(), |tile| {
            let (live, kills) = (&live, &kills);
            async move {
                stop_worker_with(
                    &tile,
                    |_| live.load(Ordering::SeqCst),
                    |_| { kills.fetch_add(1, Ordering::SeqCst); live.store(false, Ordering::SeqCst); },
                ).await
            }
        }).await;

        assert!(result.is_ok());
        assert_eq!(kills.load(Ordering::SeqCst), 1);
        assert!(!live.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn f12_stop_worker_skips_kill_when_already_gone() {
        let kills = AtomicU32::new(0);
        let stopped = stop_worker_with("tile", |_| false, |_| { kills.fetch_add(1, Ordering::SeqCst); }).await;
        assert!(stopped);
        assert_eq!(kills.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn native_binding_replays_and_resume_adopts_the_previous_worktree() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let dir = tmp_dir("native-resume");
        let repo = dir.to_str().unwrap();
        let home = dir.join("codex");
        let sessions = home.join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let rollout = sessions.join("rollout-test-native-id.jsonl");
        std::fs::write(&rollout, "{}\n").unwrap();

        for args in [&["init"][..], &["-c", "user.name=Test", "-c", "user.email=test@test.local",
            "commit", "--allow-empty", "-m", "initial"][..]] {
            assert!(std::process::Command::new("git").args(["-C", repo]).args(args).status().unwrap().success());
        }
        let head = git::head_sha(repo).unwrap();
        let run_id = format!("r-native-{}", unix_ms());
        let dispatch_id = format!("{run_id}.t1.1");
        let tile = format!("{run_id}-tile");
        let path = git::worktree_path(repo, &run_id, "t1", 1);
        let branch = git::worktree_branch(&run_id, "t1", 1);
        git::worktree_add(repo, &path, &branch, &head).unwrap();
        let run_cmd = |cmd| {
            assert!(matches!(commit_sync_raw(|_| Ok(cmd), &run_id, &base), CommitResult::Ok(_)));
        };
        run_cmd(Cmd::RunCreate {
            id: run_id.clone(), title: "test".into(), repo: repo.into(), base_ref: None,
            base_sha: Some(head), setup: None, coordinator_tile: None, limits: Limits::default(), now: 1,
        });
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.clone(), id: "t1".into(), title: "test".into(), spec: "test".into(),
            acceptance: None, deps: vec![], profile: None, escalation: vec!["codex:test-model".into(), "claude:sonnet".into()],
            verify: vec!["verify".into()], role: None, review: false, owner: None, now: 2,
        }.with_fields(serde_json::json!({"routing": 1})));
        run_cmd(Cmd::Dispatch {
            run_id: run_id.clone(), task_id: "t1".into(), id: dispatch_id.clone(), attempt: 1,
            retry_of: None, profile: Some("codex".into()), model: Some("test-model".into()),
            tile_id: Some(tile.clone()), owner: None, merge_branch: None, now: 3,
        }.with_fields(serde_json::json!({"chainIndex": 0, "nativeHome": home})));
        run_cmd(Cmd::DispatchSetWorktree {
            run_id: run_id.clone(), dispatch_id: dispatch_id.clone(), worktree: path.clone(),
            branch: branch.clone(), now: 4,
        });
        let bind = |id: &str| {
            crate::bind_agent_session(&tile, &serde_json::json!({"agent":"codex","sessionId":id,"cwd":path}).to_string());
        };
        bind("native-id");
        bind("later-id");
        let mut replayed = State::default();
        let event_path = run_events_path(&base, &run_id);
        load_into(&mut replayed, &event_path, &run_id);
        let previous = &replayed.runs[&run_id].dispatches[&dispatch_id];
        assert_eq!(previous.native_session.as_ref().unwrap().id, "native-id");
        assert_eq!(std::fs::read_to_string(event_path).unwrap().matches("dispatch-bind-session").count(), 1);

        run_cmd(Cmd::DispatchUpdate {
            run_id: run_id.clone(), dispatch_id: dispatch_id.clone(), status: DispatchStatus::Failed,
            summary: None, now: 5,
        }.with_fields(serde_json::json!({"routedFailureClass": "quota_exhausted"})));
        run_cmd(Cmd::TaskWait {
            run_id: run_id.clone(), task_id: "t1".into(),
            wait: WaitRequest { kind: WaitKind::Quota, resume_at: Some(100), reason: "quota".into(),
                candidate: wiring::dispatch_key(previous), same_session: false },
            now: 6,
        });
        run_cmd(Cmd::TaskResume {
            run_id: run_id.clone(), task_id: "t1".into(), resume_id: "t1:1:100".into(), now: 100,
        });
        let (old, mut next) = {
            let guard = state().lock().unwrap();
            let entry = &guard.runs[&run_id];
            let task = &entry.tasks["t1"];
            assert_eq!((task.quality_attempts, task.operational_attempts), (0, 1));
            let planned = f12_expect_dispatch(wiring::plan_dispatch(wiring::PlanInput {
                entry, task_id: "t1", explicit_profile: None, explicit_model: None,
                by: DispatchBy::Brain, resumed: false, now: 100,
            }, &candidates::CandidateStates::default()).unwrap());
            let old = entry.dispatches[&dispatch_id].clone();
            let mut next = old.clone();
            next.attempt = 2;
            next.worktree = None;
            next.branch = None;
            next.profile = Some(planned.profile);
            next.model = planned.model;
            next.trigger = Some(planned.trigger);
            (old, next)
        };
        let adopted = prepare_native_resume(repo, &run_id, &next, "codex").await.unwrap();
        assert_eq!(adopted.worktree.as_deref(), Some(path.as_str()));
        assert_eq!(adopted.branch.as_deref(), Some(branch.as_str()));

        for changed in [
            Dispatch { profile: Some("claude".into()), ..next.clone() },
            Dispatch { model: Some("other".into()), ..next.clone() },
            Dispatch { account: Some("other".into()), ..next.clone() },
            Dispatch { native_home: Some("other".into()), ..next.clone() },
            Dispatch { worktree: Some("other".into()), ..next.clone() },
        ] {
            assert!(native_resume_id(&old, &changed, "codex").is_none());
        }
        next.trigger = Some(Trigger::Escalation);
        next.corrective_retry = true;
        let old = Dispatch { retry_same_entry: true, ..old };
        assert_eq!(native_resume_id(&old, &next, "codex"), Some("native-id"));
        let unbound = Dispatch { native_session: None, ..old.clone() };
        assert!(native_resume_id(&unbound, &next, "codex").is_none());
        std::fs::remove_file(rollout).unwrap();
        assert!(native_resume_id(&old, &next, "codex").is_none());
    }

    #[tokio::test]
    async fn first_verification_retry_uses_the_same_entry_then_escalates() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let run_id = format!("r-corrective-{}", unix_ms());
        let mut initial = f12_state(F12Task {
            escalation: &["codex:native-first", "claude:sonnet"], profile: None, pinned: false, routing: 1,
        });
        let (first, _) = f12_dispatch(&mut initial, &candidates::CandidateStates::default());
        {
            let mut guard = state().lock().unwrap();
            let mut entry = initial.runs.remove("r1").unwrap();
            entry.run.id = run_id.clone();
            entry.tasks.get_mut("t1").unwrap().verify.push("verify".into());
            guard.runs.insert(run_id.clone(), entry);
        }

        let fail_verification = |id: &str| {
            let run_cmd = |cmd| {
                assert!(matches!(commit_sync_raw(|_| Ok(cmd), &run_id, &base), CommitResult::Ok(_)));
            };
            run_cmd(Cmd::Send {
                run_id: run_id.clone(), seq: 1, from: id.into(), to: vec!["coordinator".into()],
                msg_type: "worker_done".into(), task_id: Some("t1".into()), dispatch_id: Some(id.into()),
                reply_to: None, body: None, outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: 5,
            });
            run_cmd(Cmd::TaskVerificationFailed {
                run_id: run_id.clone(), task_id: "t1".into(), dispatch_id: id.into(),
                command: "verify".into(), exit_code: Some(1), timed_out: false, tail: "failure".into(), msg_seq: 2, now: 6,
            }.with_fields(serde_json::json!({"routedFailureClass": "quality_failed"})));
        };

        fail_verification(&first);
        let response = handle_run_update(serde_json::json!({"runId":run_id,"correctiveRetry":false}).to_string().as_bytes()).await;
        assert!(serde_json::from_str::<Value>(&response).unwrap()["ok"] == true);
        {
            let guard = state().lock().unwrap();
            let request = serde_json::json!({});
            let plan = wiring::plan_dispatch(plan_input(&guard.runs[&run_id], "t1", &request, "coordinator"),
                &candidates::CandidateStates::default()).unwrap();
            assert_eq!(f12_expect_dispatch(plan).profile, "claude");
        }
        let response = handle_run_update(serde_json::json!({"runId":run_id,"correctiveRetry":true}).to_string().as_bytes()).await;
        assert!(serde_json::from_str::<Value>(&response).unwrap()["ok"] == true);
        let retry = serde_json::json!({"runId":run_id,"taskId":"t1"}).to_string();
        let second: Value = serde_json::from_str(&handle_retry(retry.as_bytes()).await).unwrap();
        assert_eq!(second["dispatch"]["profile"], "codex", "{second}");
        assert_eq!(second["dispatch"]["chainIndex"], 0);
        assert_eq!(second["dispatch"]["correctiveRetry"], true);
        fail_verification(second["dispatch"]["id"].as_str().unwrap());
        let third: Value = serde_json::from_str(&handle_retry(retry.as_bytes()).await).unwrap();
        assert_eq!(third["dispatch"]["profile"], "claude", "{third}");
        let guard = state().lock().unwrap();
        let task = &guard.runs[&run_id].tasks["t1"];
        assert_eq!((task.attempts, task.quality_attempts, task.operational_attempts), (3, 2, 0));
    }

    #[test]
    fn f12_candidates_file_is_reloaded_by_the_init_loader() {
        let dir = std::env::temp_dir().join(format!("panorama-f12-reload-{}-{}", std::process::id(), unix_ms()));
        let key = candidates::CandidateKey::from_chain_entry("codex:f12-reload-model");
        let now = unix_ms() as i64;
        let mut saved = candidates::CandidateStates::default();
        assert!(saved.mark(candidates::MarkInput {
            key: &key, class: FailureClass::RateLimited, reset_at: Some(now + F12_HOUR),
            source: "test", confidence: candidates::Confidence::Reliable, now,
        }));
        candidates::save(&dir, &saved).expect("save candidates.json");

        let target = Mutex::new(candidates::CandidateStates::default());
        restore_candidates(&dir, &target);

        let loaded = target.lock().unwrap_or_else(|e| e.into_inner());
        assert!(!loaded.lookup(&key, now).state.is_eligible());
        assert_eq!(loaded.entries(now), saved.entries(now));
        assert!(loaded.lookup(&key, now + F12_HOUR + 1).state.is_eligible());
    }

    #[test]
    fn f12_older_candidate_snapshot_never_overwrites_a_newer_one() {
        let dir = std::env::temp_dir().join(format!("panorama-f12-persist-{}-{}", std::process::id(), unix_ms()));
        let now = unix_ms() as i64;
        let mut states = candidates::CandidateStates::default();
        let older = snapshot_candidates(&states);
        states.mark(candidates::MarkInput {
            key: &candidates::CandidateKey::from_chain_entry("codex:f12-order-model"),
            class: FailureClass::RateLimited, reset_at: Some(now + F12_HOUR),
            source: "test", confidence: candidates::Confidence::Reliable, now,
        });
        let newer = snapshot_candidates(&states);
        assert!(newer.version > older.version);

        let persisted = Mutex::new(0);
        persist_candidates_at(&dir, &newer, &persisted).expect("newer");
        persist_candidates_at(&dir, &older, &persisted).expect("older is skipped");

        assert_eq!(candidates::load(&dir).entries(now), newer.states.entries(now));
        assert_eq!(candidates::load(&dir).entries(now).len(), 1);
    }

    fn replay_task_attempt(run_id: &str, base: &std::path::Path, verify: &[&str], reported: bool) {
        let dispatch_id = format!("{run_id}.t1.1");
        let run_cmd = |cmd: Cmd| {
            assert!(matches!(commit_sync_raw(|_| Ok(cmd), run_id, base), CommitResult::Ok(_)));
        };
        run_cmd(Cmd::RunCreate {
            id: run_id.into(), title: "test".into(), repo: String::new(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            limits: Limits::default(), now: 1,
        }.with_fields(serde_json::json!({"detached": true, "autonomy": {"integration": true}})));
        run_cmd(Cmd::TaskCreate {
            run_id: run_id.into(), id: "t1".into(), title: "test".into(), spec: "test".into(),
            acceptance: None, deps: vec![], profile: Some("codex".into()), escalation: vec![],
            verify: verify.iter().map(|cmd| (*cmd).into()).collect(), role: None, review: false,
            owner: None, now: 2,
        }.with_fields(serde_json::json!({"routing": 1})));
        run_cmd(Cmd::Dispatch {
            run_id: run_id.into(), task_id: "t1".into(), id: dispatch_id.clone(), attempt: 1,
            retry_of: None, profile: Some("codex".into()), model: None, tile_id: None, owner: None,
            merge_branch: None, now: 3,
        }.with_fields(serde_json::json!({"chainIndex": 0})));
        if reported {
            run_cmd(Cmd::Send {
                run_id: run_id.into(), seq: 1, from: dispatch_id.clone(), to: vec!["coordinator".into()],
                msg_type: "worker_done".into(), task_id: Some("t1".into()), dispatch_id: Some(dispatch_id),
                reply_to: None, body: None, outcome: Some("succeeded".into()), files_modified: vec![],
                summary: None, now: 4,
            });
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn failed_auto_integrate_is_reported_once_and_coordinator_retry_clears_it() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let run_id = format!("r-auto-integrate-failure-{}", unix_ms());
        replay_task_attempt(&run_id, &base, &[], true);
        let dispatch_id = format!("{run_id}.t1.1");
        let invalid_repo = base.join("nonexistent-auto-integration-repo");
        {
            let mut guard = state().lock().unwrap();
            let entry = guard.runs.get_mut(&run_id).unwrap();
            entry.run.repo = invalid_repo.to_string_lossy().into_owned();
            entry.dispatches.get_mut(&dispatch_id).unwrap().branch = Some("missing-branch".into());
        }
        for _ in 0..2 {
            autonomy::tick_run(&run_id, unix_ms() as i64).await;
        }
        {
            let guard = state().lock().unwrap();
            let entry = &guard.runs[&run_id];
            assert_eq!(entry.auto_action_failures["t1"].action, "integrate");
            assert!(entry.auto_action_failures["t1"].error.contains("integration worktree create failed"));
            assert!(autonomy::plan(autonomy::TickInput { entry, now: unix_ms() as i64, is_live: &|_| false }).is_empty());
            let messages: Vec<_> = entry.messages.iter().filter(|m| m.msg_type == "run_idle").collect();
            assert_eq!(messages.len(), 1);
            let digest = messages[0].body.as_deref().unwrap();
            assert!(digest.contains("t1 (test)"));
            assert!(digest.contains("integration worktree create failed"));
        }

        let mut replayed = State::default();
        load_into(&mut replayed, &run_events_path(&base, &run_id), &run_id);
        assert_eq!(replayed.runs[&run_id].auto_action_failures["t1"].action, "integrate");
        let body = serde_json::json!({"runId": run_id, "taskId": "t1"});
        handle_retry(body.to_string().as_bytes()).await;
        assert!(state().lock().unwrap().runs[&run_id].auto_action_failures.is_empty());
        load_into(&mut replayed, &run_events_path(&base, &run_id), &run_id);
        assert!(replayed.runs[&run_id].auto_action_failures.is_empty());
    }

    #[test]
    fn failed_continue_does_not_leave_a_running_dispatch() {
        let base = std::env::temp_dir().join(format!("panorama-continue-failure-{}-{}", std::process::id(), unix_ms()));
        let run_id = format!("r-continue-failure-{}", unix_ms());
        replay_task_attempt(&run_id, &base, &[], false);
        let cmd = Cmd::TaskAutoActionFailure {
            run_id: run_id.clone(), task_id: "t1".into(),
            failure: Some(("continue", "worker tile closed".into())),
            expected: Some((TaskStatus::Dispatched, 3)), now: 4,
        };
        assert!(matches!(commit_sync(|_| Ok(cmd), &run_id, &base), CommitResult::Ok(_)));
        let mut replayed = State::default();
        load_into(&mut replayed, &run_events_path(&base, &run_id), &run_id);
        let entry = &replayed.runs[&run_id];
        assert_eq!(entry.running_dispatch_count(), 0);
        assert_eq!(entry.tasks["t1"].status, TaskStatus::Failed);
        assert_eq!(entry.auto_action_failures["t1"].error, "worker tile closed");
        assert!(entry.messages.iter().any(|msg| msg.msg_type == "run_idle"));
    }

    #[test]
    fn classification_survives_log_cut_after_failure_event() {
        let base = std::env::temp_dir().join(format!("panorama-crash-cut-{}-{}", std::process::id(), unix_ms()));
        for (source, class) in [
            ("worker", FailureClass::QualityFailed),
            ("supervision", FailureClass::ExecutionFailed),
            ("verification", FailureClass::QualityFailed),
        ] {
            let run_id = format!("r-crash-cut-{source}-{}", unix_ms());
            let dispatch_id = format!("{run_id}.t1.1");
            let verify = source == "verification";
            replay_task_attempt(&run_id, &base, &["verify"], verify);
            let failure = DispatchFailure { classification: f12_classification(class, None), routing: 1 };
            let cmd = match source {
                "worker" => Cmd::Send {
                    run_id: run_id.clone(), seq: 1, from: dispatch_id.clone(), to: vec!["coordinator".into()],
                    msg_type: "worker_done".into(), task_id: Some("t1".into()), dispatch_id: Some(dispatch_id.clone()),
                    reply_to: None, body: None, outcome: Some("failed".into()), files_modified: vec![],
                    summary: None, now: 5,
                },
                "supervision" => Cmd::DispatchUpdate {
                    run_id: run_id.clone(), dispatch_id: dispatch_id.clone(),
                    status: DispatchStatus::Failed, summary: None, now: 5,
                },
                _ => Cmd::TaskVerificationFailed {
                    run_id: run_id.clone(), task_id: "t1".into(), dispatch_id: dispatch_id.clone(),
                    command: "verify".into(), exit_code: Some(1), timed_out: false, tail: String::new(),
                    msg_seq: 2, now: 5,
                },
            };
            assert!(matches!(commit_sync_raw(|_| Ok(with_failure_class(cmd, Some(&failure))), &run_id, &base), CommitResult::Ok(_)));
            let mut replayed = State::default();
            load_into(&mut replayed, &run_events_path(&base, &run_id), &run_id);
            let entry = &replayed.runs[&run_id];
            let task = &entry.tasks["t1"];
            assert_eq!(entry.dispatches[&dispatch_id].failure_class, Some(class));
            assert_eq!((task.status.clone(), task.last_failure), (TaskStatus::Failed, Some(class)));
            assert_eq!(task.quality_attempts + task.operational_attempts, 1);
            if class.counter() == FailureCounter::Quality {
                assert_eq!(task.quality_floor, 1);
            }
            assert!(sync_idle_locked(&mut replayed, &run_id, &base));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn verification_spawn_failure_is_committed_and_classified() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let run_id = format!("r-verify-spawn-failure-{}", unix_ms());
        replay_task_attempt(&run_id, &base, &["echo verify"], true);
        let dispatch_id = format!("{run_id}.t1.1");
        let flight = VerificationFlight::for_report(&run_id, &dispatch_id).unwrap();
        run_verification(run_id.clone(), "t1".into(), flight, None).await;
        let guard = state().lock().unwrap();
        let entry = &guard.runs[&run_id];
        let result = &entry.dispatches[&dispatch_id].verifications[0];
        assert_eq!(result.exit_code, None);
        assert!(result.tail.iter().any(|line| line.contains("cwd, cmd and tile required")));
        assert_eq!(entry.tasks["t1"].last_failure, Some(FailureClass::QualityFailed));
        assert!(entry.messages.iter().any(|m| m.msg_type == "verification_failed"));
        assert!(!VerificationFlight::contains(&dispatch_id));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn startup_restarts_replayed_verification_once() {
        f12_test_home();
        let base = crate::panorama_dir().unwrap();
        let run_id = format!("r-verify-replay-{}", unix_ms());
        replay_task_attempt(&run_id, &base, &["echo verify"], true);
        let mut replayed = State::default();
        load_into(&mut replayed, &run_events_path(&base, &run_id), &run_id);
        let entry = &replayed.runs[&run_id];
        assert_eq!(entry.tasks["t1"].status, TaskStatus::Verifying);
        let idle = autonomy::idle_reason(entry, unix_ms() as i64).unwrap();
        assert_eq!(idle.tasks.verifying[0].id, "t1");

        let handle = {
            let mut guard = state().lock().unwrap();
            guard.runs.extend(replayed.runs);
            let entry = &guard.runs[&run_id];
            let handles: Vec<_> = restart_verifications(entry).collect();
            assert_eq!(handles.len(), 1);
            assert!(autonomy::idle_reason(entry, unix_ms() as i64).is_none());
            assert!(restart_verifications(entry).next().is_none());
            handles.into_iter().next().unwrap()
        };
        handle.await.unwrap();

        let guard = state().lock().unwrap();
        assert_eq!(guard.runs[&run_id].dispatches[&format!("{run_id}.t1.1")].verifications.len(), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn f12_second_record_failure_for_a_dispatch_counts_once() {
        f12_test_home();
        let run_id = format!("r-f12-twice-{}", unix_ms());
        f12_global_run(&run_id, "claude", None);
        let dispatch_id = format!("{run_id}.t1.1");
        let failure = classify_dispatch(&run_id, &dispatch_id, "session_gone", &[]).expect("classified");

        for _ in 0..2 {
            let run = run_id.clone();
            let dispatch = dispatch_id.clone();
            let failure = failure.clone();
            let _ = commit_deferred_idle(move |_| Ok(with_failure_class(Cmd::DispatchUpdate {
                run_id: run, dispatch_id: dispatch, status: DispatchStatus::Failed,
                summary: None, now: unix_ms(),
            }, Some(&failure))), run_id.clone()).await;
        }

        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let t = &guard.runs[&run_id].tasks["t1"];
        assert_eq!((t.operational_attempts, t.quality_attempts), (1, 0));
    }

    #[test]
    fn settings_set_and_get_role_efforts() {
        f12_test_home();
        let set_body = serde_json::json!({
            "roles": { "implement": ["claude:haiku", "claude:sonnet"] },
            "roleEfforts": {
                "implement": {
                    "claude:haiku": "low",
                    "claude:sonnet": "high"
                }
            }
        });
        let resp: Value = serde_json::from_str(&handle_settings_set(set_body.to_string().as_bytes())).unwrap();
        assert_eq!(resp["ok"], true, "{resp}");
        assert_eq!(resp["settings"]["roleEfforts"]["implement"]["claude:haiku"], "low");
        assert_eq!(resp["settings"]["roleEfforts"]["implement"]["claude:sonnet"], "high");

        let get_resp: Value = serde_json::from_str(&handle_settings_get()).unwrap();
        assert_eq!(get_resp["settings"]["roleEfforts"]["implement"]["claude:sonnet"], "high");

        let unset_body = serde_json::json!({
            "roleEfforts": { "implement": { "claude:haiku": null } }
        });
        let unset_resp: Value = serde_json::from_str(&handle_settings_set(unset_body.to_string().as_bytes())).unwrap();
        assert_eq!(unset_resp["ok"], true, "{unset_resp}");
        assert!(unset_resp["settings"]["roleEfforts"]["implement"]["claude:haiku"].is_null());
        assert_eq!(unset_resp["settings"]["roleEfforts"]["implement"]["claude:sonnet"], "high");

        let bad_body = serde_json::json!({
            "roleEfforts": { "implement": { "claude:sonnet": "turbo" } }
        });
        let bad_resp: Value = serde_json::from_str(&handle_settings_set(bad_body.to_string().as_bytes())).unwrap();
        assert_eq!(bad_resp["ok"], false);
        assert_eq!(bad_resp["error"]["code"], "bad_effort");

        let prune_body = serde_json::json!({
            "roles": { "implement": ["claude:haiku"] }
        });
        let prune_resp: Value = serde_json::from_str(&handle_settings_set(prune_body.to_string().as_bytes())).unwrap();
        assert_eq!(prune_resp["ok"], true, "{prune_resp}");
        assert!(prune_resp["settings"]["roleEfforts"].get("implement").is_none());
    }
}

mod failure;
