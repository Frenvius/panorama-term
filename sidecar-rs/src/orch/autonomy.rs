use serde_json::{json, Value};
use crate::orch::model::*;
use crate::orch::reducer::{Cmd, RunEntry};
use crate::orch::wiring;
use super::{
    commit, handle_integrate, handle_retry, handle_worker_start, send_coordinator_notice, state,
    supervision_watch, tail_lines, unix_ms, CommitResult,
};

const COORDINATOR_RESUME_GRACE_MS: i64 = 60_000;
const NOTICE_TASK_LIMIT: usize = 10;
const SCREEN_TAIL_LINES: usize = 40;
const CONTINUE_TEXT: &str = "continue";

#[derive(Debug, PartialEq)]
pub(super) struct Revive {
    pub dispatch_id: String,
    pub tile_id: String,
}

#[derive(Debug, PartialEq)]
pub(super) enum Action {
    Retry { task_id: String, stop_live: bool, reason: &'static str },
    Start { task_id: String, retry_of: Option<String>, stop_live: bool, reason: &'static str },
    Resume { task_id: String, resume_id: String, revive: Option<Revive> },
    ResumeReady { task_id: String, dispatch_id: Option<String>, body: String },
    Integrate { task_id: String },
}

pub(super) struct TickInput<'a> {
    pub entry: &'a RunEntry,
    pub now: i64,
    pub is_live: &'a dyn Fn(&str) -> bool,
}

struct TaskCtx<'a> {
    entry: &'a RunEntry,
    task: &'a Task,
    now: i64,
    is_live: &'a dyn Fn(&str) -> bool,
}

fn take_slot(slots: &mut usize) -> bool {
    if *slots == 0 {
        return false;
    }
    *slots -= 1;
    true
}

pub(super) fn plan(input: TickInput) -> Vec<Action> {
    let TickInput { entry, now, is_live } = input;
    if entry.run.status != RunStatus::Active {
        return Vec::new();
    }
    let mut slots = (entry.run.limits.max_workers as usize).saturating_sub(entry.running_dispatch_count());
    let mut tasks: Vec<&Task> = entry.tasks.values().collect();
    tasks.sort_by_key(|t| (t.id.len(), t.id.clone()));
    let mut actions = Vec::new();
    for task in tasks {
        if task.owner.as_deref() == Some("user") {
            continue;
        }
        let ctx = TaskCtx { entry, task, now, is_live };
        let planned = match task.status {
            TaskStatus::Waiting => plan_waiting(&ctx, &mut slots),
            TaskStatus::Failed => plan_failed(&ctx, &mut slots),
            TaskStatus::Pending => plan_pending(&ctx, &mut slots),
            TaskStatus::Completed => plan_integrate(&ctx),
            _ => None,
        };
        actions.extend(planned);
    }
    actions
}

fn has_counter(task: &Task, counter: FailureCounter) -> bool {
    task.last_failure.is_some_and(|class| class.counter() == counter)
}

fn failure_recorded(ctx: &TaskCtx) -> bool {
    ctx.entry.latest_dispatch_for_task(&ctx.task.id).is_some_and(|d| d.failure_class.is_some())
}

fn has_open_gate(entry: &RunEntry, task: &Task) -> bool {
    entry.gates.values().any(|g| g.task_id == task.id && g.resolution.is_none())
}

fn has_chain(task: &Task) -> bool {
    !task.escalation.is_empty() || task.profile.is_some()
}

fn deps_integrated(entry: &RunEntry, task: &Task) -> bool {
    task.deps.iter().all(|dep| {
        entry.tasks.get(dep).is_some_and(|d| d.status == TaskStatus::Completed && d.integrated_sha.is_some())
    })
}

fn revive_target(ctx: &TaskCtx, wait: &TaskWait) -> Option<Revive> {
    if !wait.same_session {
        return None;
    }
    let latest = ctx.entry.latest_dispatch_for_task(&ctx.task.id)?;
    if latest.status != DispatchStatus::Failed || latest.outcome.is_some() {
        return None;
    }
    let tile_id = latest.tile_id.clone().filter(|t| (ctx.is_live)(t))?;
    Some(Revive { dispatch_id: latest.id.clone(), tile_id })
}

fn resume_ready_body(wait: &TaskWait) -> String {
    json!({
        "resumeId": wait.resume_id,
        "kind": wait.kind,
        "resumeAt": wait.resume_at,
        "reason": wait.reason,
        "hint": "resume it: task-update --status pending, then worker-start",
    }).to_string()
}

fn plan_waiting(ctx: &TaskCtx, slots: &mut usize) -> Option<Action> {
    let wait = ctx.task.wait.as_ref().filter(|w| w.kind != WaitKind::Intervention)?;
    if !wait.resume_at.is_some_and(|at| at <= ctx.now) {
        return None;
    }
    if ctx.entry.run.autonomy.operational {
        take_slot(slots);
        return Some(Action::Resume {
            task_id: ctx.task.id.clone(),
            resume_id: wait.resume_id.clone(),
            revive: revive_target(ctx, wait),
        });
    }
    let body = resume_ready_body(wait);
    let already_sent = ctx.entry.messages.iter().any(|m| {
        m.msg_type == "resume_ready"
            && m.task_id.as_deref() == Some(ctx.task.id.as_str())
            && m.body.as_deref() == Some(body.as_str())
    });
    if already_sent {
        return None;
    }
    let dispatch_id = ctx.entry.latest_dispatch_for_task(&ctx.task.id).map(|d| d.id.clone());
    Some(Action::ResumeReady { task_id: ctx.task.id.clone(), dispatch_id, body })
}

fn plan_failed(ctx: &TaskCtx, slots: &mut usize) -> Option<Action> {
    let (entry, task) = (ctx.entry, ctx.task);
    if task.routing != 1 || !failure_recorded(ctx) {
        return None;
    }
    let autonomy = entry.run.autonomy;
    let limits = &entry.run.limits;
    if autonomy.operational
        && has_counter(task, FailureCounter::Operational)
        && task.operational_attempts < limits.max_operational_attempts
    {
        take_slot(slots);
        return Some(Action::Retry { task_id: task.id.clone(), stop_live: true, reason: "operational failure, router picks the next candidate" });
    }
    if autonomy.execution
        && has_counter(task, FailureCounter::Quality)
        && task.quality_attempts < limits.max_attempts
        && take_slot(slots)
    {
        return Some(Action::Retry { task_id: task.id.clone(), stop_live: false, reason: "quality failure, escalating past the failed candidate" });
    }
    None
}

fn plan_pending(ctx: &TaskCtx, slots: &mut usize) -> Option<Action> {
    let (entry, task) = (ctx.entry, ctx.task);
    if task.routing != 1 || !has_chain(task) || has_open_gate(entry, task) {
        return None;
    }
    let latest = entry.latest_dispatch_for_task(&task.id);
    if latest.is_some_and(|d| d.status == DispatchStatus::Running) {
        return None;
    }
    let retry_of = latest.map(|d| d.id.clone());
    let autonomy = entry.run.autonomy;
    let resumed = task.routing == 1 && task.attempts > 0 && has_counter(task, FailureCounter::Operational);
    if resumed && autonomy.operational {
        take_slot(slots);
        return Some(Action::Start { task_id: task.id.clone(), retry_of, stop_live: true, reason: "resumed after an operational wait" });
    }
    if !autonomy.execution || !deps_integrated(entry, task) || !take_slot(slots) {
        return None;
    }
    Some(Action::Start { task_id: task.id.clone(), retry_of, stop_live: false, reason: "dependencies completed and integrated" })
}

fn plan_integrate(ctx: &TaskCtx) -> Option<Action> {
    let (entry, task) = (ctx.entry, ctx.task);
    if !entry.run.autonomy.integration || task.review || task.integrated_sha.is_some() {
        return None;
    }
    let has_branch = entry.dispatches.values().any(|d| {
        d.task_id == task.id
            && d.status == DispatchStatus::Reported
            && d.outcome.as_deref() == Some("succeeded")
            && d.branch.is_some()
    });
    has_branch.then(|| Action::Integrate { task_id: task.id.clone() })
}

pub(super) struct CoordinatorProbe {
    pub live: bool,
    pub idle: bool,
    pub tail: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub(super) enum CoordinatorStep {
    Nothing,
    SetWait { reset_at: Option<i64>, reason: String },
    Resume { resume_id: String, tile_id: String, notice: String },
}

fn status_word(status: &TaskStatus) -> String {
    serde_json::to_value(status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn clock_label(at_ms: i64, offset_min: i32) -> String {
    let minutes = (at_ms / 60_000 + offset_min as i64).rem_euclid(24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

pub(super) fn coordinator_notice(entry: &RunEntry, wait: &CoordinatorWait, offset_min: i32) -> String {
    let mut changed: Vec<&Task> = entry.tasks.values().filter(|t| t.updated_at as i64 >= wait.since).collect();
    changed.sort_by_key(|t| (t.id.len(), t.id.clone()));
    let mut lines: Vec<String> = changed.iter().take(NOTICE_TASK_LIMIT)
        .map(|t| format!("{} {}", t.id, status_word(&t.status)))
        .collect();
    if changed.len() > NOTICE_TASK_LIMIT {
        lines.push(format!("+{} more", changed.len() - NOTICE_TASK_LIMIT));
    }
    if lines.is_empty() {
        lines.push("no task changes".into());
    }
    let unread = entry.messages.iter().filter(|m| !m.acked && m.to == "coordinator").count();
    format!(
        "[panorama] Coordinator resume for run {}. Since {}: {}. {} unread messages: run check.",
        entry.run.id,
        clock_label(wait.since, offset_min),
        lines.join("; "),
        unread,
    )
}

pub(super) fn coordinator_step(entry: &RunEntry, now: i64, probe: &CoordinatorProbe) -> CoordinatorStep {
    let Some(tile_id) = entry.run.coordinator_tile.clone().filter(|_| probe.live) else {
        return CoordinatorStep::Nothing;
    };
    if let Some(wait) = &entry.run.coordinator_wait {
        return match wait.reset_at {
            Some(at) if now >= at + COORDINATOR_RESUME_GRACE_MS => CoordinatorStep::Resume {
                resume_id: wait.resume_id.clone(),
                tile_id,
                notice: coordinator_notice(entry, wait, wiring::local_offset_min()),
            },
            _ => CoordinatorStep::Nothing,
        };
    }
    if !probe.idle {
        return CoordinatorStep::Nothing;
    }
    let found = wiring::classify_failure(Some("claude"), "", &probe.tail, now);
    let limited = matches!(found.class, FailureClass::QuotaExhausted | FailureClass::RateLimited);
    if !limited || found.reset_at.is_some_and(|at| at <= now) {
        return CoordinatorStep::Nothing;
    }
    CoordinatorStep::SetWait { reset_at: found.reset_at, reason: found.source }
}

#[cfg(not(test))]
fn tile_is_live(tile_id: &str) -> bool {
    crate::lookup_live(tile_id).is_some()
}

#[cfg(not(test))]
fn type_into_tile(tile_id: &str, text: &str) -> Result<(), String> {
    crate::send_to_tile(tile_id, text)
}

#[cfg(test)]
pub(super) static TEST_LIVE_TILES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
#[cfg(test)]
pub(super) static TEST_TYPED: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn tile_is_live(tile_id: &str) -> bool {
    TEST_LIVE_TILES.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|t| t == tile_id)
}

#[cfg(test)]
fn type_into_tile(tile_id: &str, text: &str) -> Result<(), String> {
    TEST_TYPED.lock().unwrap_or_else(|e| e.into_inner()).push((tile_id.to_string(), text.to_string()));
    Ok(())
}

fn skipped(run_id: &str, task_id: &str, action: &str, detail: &str) {
    if cfg!(debug_assertions) {
        eprintln!("[orch] autonomy {action} {run_id}/{task_id} skipped: {detail}");
    }
}

pub(super) async fn tick_all() {
    let run_ids: Vec<String> = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.values().filter(|e| e.run.status == RunStatus::Active).map(|e| e.run.id.clone()).collect()
    };
    for run_id in run_ids {
        tick_run(&run_id, unix_ms() as i64).await;
    }
}

pub(super) async fn tick_run(run_id: &str, now: i64) {
    let actions = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(run_id) else { return };
        plan(TickInput { entry, now, is_live: &tile_is_live })
    };
    for action in actions {
        execute(run_id, action).await;
    }
    coordinator_tick(run_id, now).await;
}

fn request_body(run_id: &str, task_id: &str, extra: Value) -> Vec<u8> {
    let mut body = json!({"runId": run_id, "taskId": task_id, "by": "brain"});
    if let (Some(obj), Some(more)) = (body.as_object_mut(), extra.as_object()) {
        obj.extend(more.iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.clone(), v.clone())));
    }
    body.to_string().into_bytes()
}

async fn announce(run_id: &str, task_id: &str, dispatch_id: Option<String>, what: (&str, &str)) {
    let body = json!({
        "action": what.0, "taskId": task_id, "dispatchId": dispatch_id, "reason": what.1,
    }).to_string();
    send_coordinator_notice(run_id.to_string(), task_id.to_string(), dispatch_id, "auto_action", body).await;
}

async fn finish_dispatching(run_id: &str, task_id: &str, what: (&str, &str), resp: &str) {
    let Ok(v) = serde_json::from_str::<Value>(resp) else { return };
    if v["ok"].as_bool() != Some(true) {
        skipped(run_id, task_id, what.0, v["error"]["code"].as_str().unwrap_or("error"));
        return;
    }
    if v.get("waiting").is_some() {
        return;
    }
    let dispatch_id = v["dispatch"]["id"].as_str().map(str::to_string);
    announce(run_id, task_id, dispatch_id, what).await;
}

fn latest_dispatch_id(run_id: &str, task_id: &str) -> Option<String> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.runs.get(run_id)?.latest_dispatch_for_task(task_id).map(|d| d.id.clone())
}

async fn execute(run_id: &str, action: Action) {
    match action {
        Action::Retry { task_id, stop_live, reason } => {
            let resp = handle_retry(&request_body(run_id, &task_id, json!({"stopLive": stop_live}))).await;
            finish_dispatching(run_id, &task_id, ("retry", reason), &resp).await;
        }
        Action::Start { task_id, retry_of, stop_live, reason } => {
            let resp = handle_worker_start(&request_body(run_id, &task_id, json!({"stopLive": stop_live, "retryOf": retry_of}))).await;
            finish_dispatching(run_id, &task_id, ("start", reason), &resp).await;
        }
        Action::Integrate { task_id } => {
            let resp = handle_integrate(&request_body(run_id, &task_id, json!({}))).await;
            let integrated = serde_json::from_str::<Value>(&resp).is_ok_and(|v| v["integrated"].as_bool() == Some(true));
            if integrated {
                announce(run_id, &task_id, None, ("integrate", "completed task merged into the integration branch")).await;
            } else {
                skipped(run_id, &task_id, "integrate", "not integrated");
            }
        }
        Action::Resume { task_id, resume_id, revive } => resume(run_id, &task_id, &resume_id, revive).await,
        Action::ResumeReady { task_id, dispatch_id, body } => {
            send_coordinator_notice(run_id.to_string(), task_id, dispatch_id, "resume_ready", body).await;
        }
    }
}

async fn revive_dispatch(run_id: &str, revive: &Revive) -> bool {
    let (run_c, dispatch_c) = (run_id.to_string(), revive.dispatch_id.clone());
    let now = unix_ms();
    let revived = commit(move |_| Ok(Cmd::DispatchRevive { run_id: run_c, dispatch_id: dispatch_c, now }), run_id.to_string()).await;
    if !matches!(revived, CommitResult::Ok(_)) {
        return false;
    }
    supervision_watch().lock().unwrap_or_else(|e| e.into_inner()).remove(&revive.dispatch_id);
    if let Err(e) = type_into_tile(&revive.tile_id, CONTINUE_TEXT) {
        skipped(run_id, &revive.dispatch_id, "continue", &e);
    }
    true
}

async fn resume(run_id: &str, task_id: &str, resume_id: &str, revive: Option<Revive>) {
    let (run_c, task_c, resume_c) = (run_id.to_string(), task_id.to_string(), resume_id.to_string());
    let now = unix_ms();
    let resumed = commit(move |_| Ok(Cmd::TaskResume { run_id: run_c, task_id: task_c, resume_id: resume_c, now }), run_id.to_string()).await;
    if !matches!(resumed, CommitResult::Ok(_)) {
        skipped(run_id, task_id, "resume", "already resumed or changed");
        return;
    }
    let reason = format!("resume {resume_id}");
    if let Some(revive) = revive {
        if revive_dispatch(run_id, &revive).await {
            announce(run_id, task_id, Some(revive.dispatch_id), ("continue", &reason)).await;
            return;
        }
    }
    let retry_of = latest_dispatch_id(run_id, task_id);
    let resp = handle_worker_start(&request_body(run_id, task_id, json!({"stopLive": true, "retryOf": retry_of, "resume": true}))).await;
    finish_dispatching(run_id, task_id, ("start", &reason), &resp).await;
}

fn probe_coordinator(tile_id: &str) -> CoordinatorProbe {
    let Some(session) = crate::lookup_live(tile_id) else {
        return CoordinatorProbe { live: false, idle: false, tail: Vec::new() };
    };
    let status = crate::cached_agent_state(&session)
        .and_then(|js| serde_json::from_str::<Value>(&js).ok())
        .and_then(|v| v["lastStatus"].as_str().map(str::to_string));
    let idle = matches!(status.as_deref(), Some("idle") | Some("waiting"));
    let tail = if idle {
        tail_lines(&super::worker::capture_worker_screen(tile_id, SCREEN_TAIL_LINES))
    } else {
        Vec::new()
    };
    CoordinatorProbe { live: true, idle, tail }
}

async fn coordinator_tick(run_id: &str, now: i64) {
    let tile = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(run_id).and_then(|e| e.run.coordinator_tile.clone())
    };
    let Some(tile) = tile else { return };
    let probe = probe_coordinator(&tile);
    apply_coordinator_step(run_id, now, &probe).await;
}

pub(super) async fn apply_coordinator_step(run_id: &str, now: i64, probe: &CoordinatorProbe) {
    let step = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(run_id) else { return };
        coordinator_step(entry, now, probe)
    };
    match step {
        CoordinatorStep::Nothing => {}
        CoordinatorStep::SetWait { reset_at, reason } => {
            let run_c = run_id.to_string();
            commit(move |_| Ok(Cmd::RunSetCoordinatorWait { run_id: run_c, reset_at, reason, now: unix_ms() }), run_id.to_string()).await;
        }
        CoordinatorStep::Resume { resume_id, tile_id, notice } => {
            let run_c = run_id.to_string();
            let cleared = commit(move |_| Ok(Cmd::RunClearCoordinatorWait { run_id: run_c, resume_id, now: unix_ms() }), run_id.to_string()).await;
            if !matches!(cleared, CommitResult::Ok(_)) {
                return;
            }
            if let Err(e) = type_into_tile(&tile_id, &notice) {
                skipped(run_id, "coordinator", "resume-notice", &e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::reducer::{ApplyResult, State};
    use super::super::tests::{f12_commit, f12_test_home};

    const NOW: i64 = 5_000_000_000;

    fn autonomy(operational: bool, execution: bool, integration: bool) -> Autonomy {
        Autonomy { operational, execution, integration }
    }

    fn new_state(run_id: &str, auto: Autonomy) -> State {
        let mut s = State::default();
        f12_commit(&mut s, Cmd::RunCreate {
            id: run_id.into(), title: "t".into(), repo: "/tmp".into(), base_ref: None, base_sha: None,
            setup: None, coordinator_tile: None, limits: Limits::default(), now: 1_000_000,
        });
        f12_commit(&mut s, Cmd::RunUpdate { run_id: run_id.into(), title: "t".into(), now: 1_000_001 }
            .with_fields(json!({"autonomy": auto})));
        s
    }

    fn add_task(s: &mut State, run_id: &str, id: &str, chain: &[&str], deps: &[&str]) {
        f12_commit(s, Cmd::TaskCreate {
            run_id: run_id.into(), id: id.into(), title: id.into(), spec: "s".into(), acceptance: None,
            deps: deps.iter().map(|d| d.to_string()).collect(),
            profile: None, escalation: chain.iter().map(|c| c.to_string()).collect(),
            verify: vec![], role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(json!({"routing": 1, "pinned": false})));
    }

    fn dispatch(s: &mut State, run_id: &str, task_id: &str, tile: Option<&str>) -> String {
        let attempt = s.runs[run_id].tasks[task_id].attempts + 1;
        let id = format!("{run_id}.{task_id}.{attempt}");
        f12_commit(s, Cmd::Dispatch { merge_branch: None,
            run_id: run_id.into(), id: id.clone(), task_id: task_id.into(), attempt, retry_of: None,
            profile: Some("codex".into()), model: Some("f12c2".into()), tile_id: tile.map(str::to_string),
            owner: None, now: 1_000_000,
        });
        id
    }

    fn fail(s: &mut State, run_id: &str, dispatch_id: &str, class: FailureClass) {
        let task_id = s.runs[run_id].dispatches[dispatch_id].task_id.clone();
        f12_commit(s, Cmd::DispatchUpdate {
            run_id: run_id.into(), dispatch_id: dispatch_id.into(), status: DispatchStatus::Failed, summary: None, now: 1_000_100,
        });
        f12_commit(s, Cmd::TaskRecordFailure {
            run_id: run_id.into(), task_id, dispatch_id: dispatch_id.into(), class, chain_index: None, now: 1_000_200,
        });
    }

    fn wait(s: &mut State, run_id: &str, task_id: &str, resume_at: Option<i64>, same_session: bool) {
        f12_commit(s, Cmd::TaskWait {
            run_id: run_id.into(), task_id: task_id.into(),
            wait: crate::orch::reducer::WaitRequest {
                kind: WaitKind::Quota, resume_at, reason: "quota".into(), candidate: None, same_session,
            },
            now: 1_000_300,
        });
    }

    fn plan_at(s: &State, run_id: &str, now: i64) -> Vec<Action> {
        plan(TickInput { entry: &s.runs[run_id], now, is_live: &|_| true })
    }

    fn install(s: State) {
        state().lock().unwrap_or_else(|e| e.into_inner()).runs.extend(s.runs);
    }

    fn unique(label: &str) -> String {
        format!("r-c2-{label}-{}", unix_ms())
    }

    fn messages_of(run_id: &str, msg_type: &str) -> Vec<Message> {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs[run_id].messages.iter().filter(|m| m.msg_type == msg_type).cloned().collect()
    }

    fn dispatch_count(run_id: &str) -> usize {
        state().lock().unwrap_or_else(|e| e.into_inner()).runs[run_id].dispatches.len()
    }

    fn failed_state(run_id: &str, auto: Autonomy, class: FailureClass) -> State {
        let mut s = new_state(run_id, auto);
        add_task(&mut s, run_id, "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut s, run_id, "t1", None);
        fail(&mut s, run_id, &d, class);
        s
    }

    #[test]
    fn plan_retries_operationally_failed_task_only_with_operational_autonomy() {
        let on = failed_state("r1", autonomy(true, false, false), FailureClass::QuotaExhausted);
        assert!(matches!(plan_at(&on, "r1", NOW).as_slice(), [Action::Retry { stop_live: true, .. }]));
        let off = failed_state("r1", autonomy(false, true, true), FailureClass::QuotaExhausted);
        assert!(plan_at(&off, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_skips_failure_whose_class_is_not_recorded_yet() {
        let mut s = new_state("r1", autonomy(true, true, true));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut s, "r1", "t1", None);
        f12_commit(&mut s, Cmd::DispatchUpdate {
            run_id: "r1".into(), dispatch_id: d, status: DispatchStatus::Failed, summary: None, now: 1_000_100,
        });
        assert!(plan_at(&s, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_respects_operational_and_quality_caps() {
        let mut s = failed_state("r1", autonomy(true, true, false), FailureClass::QuotaExhausted);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().operational_attempts = 6;
        assert!(plan_at(&s, "r1", NOW).is_empty());
        let mut q = failed_state("r1", autonomy(true, true, false), FailureClass::QualityFailed);
        assert!(matches!(plan_at(&q, "r1", NOW).as_slice(), [Action::Retry { stop_live: false, .. }]));
        q.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().quality_attempts = 3;
        assert!(plan_at(&q, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_quality_retry_needs_execution_autonomy() {
        let s = failed_state("r1", autonomy(true, false, true), FailureClass::QualityFailed);
        assert!(plan_at(&s, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_never_resumes_intervention_waits_or_unknown_resume_times() {
        let mut s = failed_state("r1", autonomy(true, true, true), FailureClass::QuotaExhausted);
        wait(&mut s, "r1", "t1", None, false);
        assert!(plan_at(&s, "r1", NOW).is_empty());
        let mut i = failed_state("r1", autonomy(true, true, true), FailureClass::SetupFailed);
        f12_commit(&mut i, Cmd::TaskWait {
            run_id: "r1".into(), task_id: "t1".into(),
            wait: crate::orch::reducer::WaitRequest {
                kind: WaitKind::Intervention, resume_at: Some(1), reason: "x".into(), candidate: None, same_session: false,
            },
            now: 1_000_300,
        });
        assert!(plan_at(&i, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_waits_for_a_future_resume_time() {
        let mut s = failed_state("r1", autonomy(true, false, false), FailureClass::QuotaExhausted);
        wait(&mut s, "r1", "t1", Some(NOW + 60_000), false);
        assert!(plan_at(&s, "r1", NOW).is_empty());
        assert!(matches!(plan_at(&s, "r1", NOW + 60_000).as_slice(), [Action::Resume { .. }]));
    }

    #[test]
    fn plan_without_operational_autonomy_sends_resume_ready_once() {
        let mut s = failed_state("r1", autonomy(false, false, false), FailureClass::QuotaExhausted);
        wait(&mut s, "r1", "t1", Some(NOW - 1), false);
        let actions = plan_at(&s, "r1", NOW);
        let [Action::ResumeReady { body, .. }] = actions.as_slice() else { panic!("expected resume_ready, got {actions:?}") };
        assert!(body.contains("t1:1:"));
        let entry = s.runs.get_mut("r1").unwrap();
        let seq = entry.next_msg_seq();
        entry.messages.push(Message {
            seq, from: "brain".into(), to: "coordinator".into(), msg_type: "resume_ready".into(),
            task_id: Some("t1".into()), dispatch_id: None, reply_to: None, body: Some(body.clone()), at: 1, acked: false,
        });
        assert!(plan_at(&s, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_execution_starts_only_tasks_whose_deps_are_integrated() {
        let mut s = new_state("r1", autonomy(false, true, false));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        add_task(&mut s, "r1", "t2", &["codex:f12c2"], &["t1"]);
        let entry = s.runs.get_mut("r1").unwrap();
        entry.tasks.get_mut("t1").unwrap().status = TaskStatus::Completed;
        assert!(plan_at(&s, "r1", NOW).is_empty());
        let entry = s.runs.get_mut("r1").unwrap();
        entry.tasks.get_mut("t1").unwrap().integrated_sha = Some("abc".into());
        let actions = plan_at(&s, "r1", NOW);
        assert!(matches!(actions.as_slice(), [Action::Start { task_id, .. }] if task_id == "t2"));
    }

    #[test]
    fn plan_execution_never_starts_legacy_pending_tasks() {
        let mut s = new_state("r1", autonomy(false, true, false));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().routing = 0;
        assert!(plan_at(&s, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_execution_respects_max_workers() {
        let mut s = new_state("r1", autonomy(false, true, false));
        for id in ["t1", "t2", "t3"] {
            add_task(&mut s, "r1", id, &["codex:f12c2"], &[]);
        }
        s.runs.get_mut("r1").unwrap().run.limits.max_workers = 2;
        assert_eq!(plan_at(&s, "r1", NOW).len(), 2);
        dispatch(&mut s, "r1", "t1", None);
        assert_eq!(plan_at(&s, "r1", NOW).len(), 1);
    }

    #[test]
    fn plan_execution_skips_user_owned_and_gated_tasks() {
        let mut s = new_state("r1", autonomy(true, true, true));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        add_task(&mut s, "r1", "t2", &["codex:f12c2"], &[]);
        add_task(&mut s, "r1", "t3", &["codex:f12c2"], &[]);
        f12_commit(&mut s, Cmd::GateCreate {
            run_id: "r1".into(), id: "g1".into(), task_id: "t2".into(), question: "q".into(),
            options: vec![], msg_seq: 1, now: 1_000_400,
        });
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().owner = Some("user".into());
        let actions = plan_at(&s, "r1", NOW);
        assert!(matches!(actions.as_slice(), [Action::Start { task_id, .. }] if task_id == "t3"), "{actions:?}");
    }

    #[test]
    fn plan_ignores_paused_runs_and_tasks_without_a_chain() {
        let mut s = new_state("r1", autonomy(true, true, true));
        add_task(&mut s, "r1", "t1", &[], &[]);
        assert!(plan_at(&s, "r1", NOW).is_empty());
        add_task(&mut s, "r1", "t2", &["codex:f12c2"], &[]);
        s.runs.get_mut("r1").unwrap().run.status = RunStatus::Paused;
        assert!(plan_at(&s, "r1", NOW).is_empty());
    }

    #[test]
    fn plan_integrates_completed_unreviewed_tasks_with_a_reported_branch() {
        let mut s = new_state("r1", autonomy(false, false, true));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut s, "r1", "t1", None);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Completed;
        assert!(plan_at(&s, "r1", NOW).is_empty());
        let disp = s.runs.get_mut("r1").unwrap().dispatches.get_mut(&d).unwrap();
        disp.status = DispatchStatus::Reported;
        disp.outcome = Some("succeeded".into());
        disp.branch = Some("orch/r1/t1-1".into());
        assert!(matches!(plan_at(&s, "r1", NOW).as_slice(), [Action::Integrate { .. }]));
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().review = true;
        assert!(plan_at(&s, "r1", NOW).is_empty());
    }

    #[test]
    fn restart_replay_then_first_tick_acts_on_a_past_resume_time() {
        let mut s = State::default();
        let mut log: Vec<(String, Value)> = Vec::new();
        let mut apply = |s: &mut State, cmd: Cmd| log.push(f12_commit(s, cmd));
        apply(&mut s, Cmd::RunCreate {
            id: "r1".into(), title: "t".into(), repo: "/tmp".into(), base_ref: None, base_sha: None,
            setup: None, coordinator_tile: None, limits: Limits::default(), now: 1_000_000,
        });
        apply(&mut s, Cmd::RunUpdate { run_id: "r1".into(), title: "t".into(), now: 1 }
            .with_fields(json!({"autonomy": autonomy(true, false, false)})));
        apply(&mut s, Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "t".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec!["codex:f12c2".into()],
            verify: vec![], role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(json!({"routing": 1, "pinned": false})));
        apply(&mut s, Cmd::Dispatch { merge_branch: None,
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(), attempt: 1, retry_of: None,
            profile: Some("codex".into()), model: None, tile_id: None, owner: None, now: 1_000_000,
        });
        apply(&mut s, Cmd::DispatchUpdate {
            run_id: "r1".into(), dispatch_id: "r1.t1.1".into(), status: DispatchStatus::Failed, summary: None, now: 2,
        });
        apply(&mut s, Cmd::TaskRecordFailure {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            class: FailureClass::QuotaExhausted, chain_index: None, now: 3,
        });
        apply(&mut s, Cmd::TaskWait {
            run_id: "r1".into(), task_id: "t1".into(),
            wait: crate::orch::reducer::WaitRequest {
                kind: WaitKind::Quota, resume_at: Some(NOW - 5_000), reason: "quota".into(), candidate: None, same_session: false,
            },
            now: 4,
        });
        let mut fresh = State::default();
        for (kind, data) in &log {
            fresh.replay(kind, data).expect("replay");
        }
        let actions = plan_at(&fresh, "r1", NOW);
        assert!(matches!(actions.as_slice(), [Action::Resume { task_id, resume_id, .. }] if task_id == "t1" && resume_id == "t1:1:4999995000"), "{actions:?}");
    }

    #[test]
    fn dispatch_revive_requires_a_failed_live_attempt_on_a_pending_task() {
        let mut s = failed_state("r1", autonomy(true, false, false), FailureClass::QuotaExhausted);
        let revive = || Cmd::DispatchRevive { run_id: "r1".into(), dispatch_id: "r1.t1.1".into(), now: 9 };
        assert!(matches!(s.apply(&revive()), ApplyResult::Err(_)), "task is failed, not pending");
        f12_commit(&mut s, Cmd::TaskUpdate { owner: None, by: "test".into(), reason: None,
            run_id: "r1".into(), task_id: "t1".into(), status: Some(TaskStatus::Pending), result: None,
            spec: None, acceptance: None, verify: None, profile: None, escalation: None, review: None, now: 5,
        });
        f12_commit(&mut s, revive());
        let entry = &s.runs["r1"];
        assert_eq!(entry.dispatches["r1.t1.1"].status, DispatchStatus::Running);
        assert_eq!(entry.dispatches["r1.t1.1"].failure_class, None);
        assert_eq!(entry.tasks["t1"].status, TaskStatus::Dispatched);
        assert!(matches!(s.apply(&revive()), ApplyResult::Err(_)), "revive is not repeatable");
    }

    #[test]
    fn coordinator_wait_commands_are_guarded_and_replayable() {
        let mut s = new_state("r1", Autonomy::default());
        let set = |reset_at| Cmd::RunSetCoordinatorWait { run_id: "r1".into(), reset_at, reason: "quota".into(), now: 7 };
        let (kind, data) = f12_commit(&mut s, set(Some(100)));
        assert_eq!(s.runs["r1"].run.coordinator_wait.as_ref().map(|w| w.resume_id.as_str()), Some("r1:coordinator:100"));
        assert!(matches!(s.apply(&set(Some(200))), ApplyResult::Err(_)), "one wait at a time");
        let clear = |id: &str| Cmd::RunClearCoordinatorWait { run_id: "r1".into(), resume_id: id.into(), now: 8 };
        assert!(matches!(s.apply(&clear("r1:coordinator:999")), ApplyResult::Err(_)));
        let mut fresh = State::default();
        fresh.replay("run-create", &json!({"id": "r1", "title": "t", "repo": "/tmp", "limits": Limits::default(), "now": 1})).unwrap();
        fresh.replay(&kind, &data).unwrap();
        assert!(fresh.runs["r1"].run.coordinator_wait.is_some());
        f12_commit(&mut s, clear("r1:coordinator:100"));
        assert!(s.runs["r1"].run.coordinator_wait.is_none());
        assert!(matches!(s.apply(&clear("r1:coordinator:100")), ApplyResult::Err(_)), "typed once");
    }

    #[test]
    fn coordinator_notice_lists_changed_tasks_and_unread_count() {
        let mut s = new_state("r1", Autonomy::default());
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        add_task(&mut s, "r1", "t2", &["codex:f12c2"], &[]);
        let entry = s.runs.get_mut("r1").unwrap();
        entry.tasks.get_mut("t1").unwrap().updated_at = 3_700_000;
        entry.tasks.get_mut("t1").unwrap().status = TaskStatus::Completed;
        entry.tasks.get_mut("t2").unwrap().updated_at = 10;
        entry.messages.push(Message {
            seq: 1, from: "d".into(), to: "coordinator".into(), msg_type: "status".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None, at: 1, acked: false,
        });
        let wait = CoordinatorWait { reset_at: Some(1), since: 3_600_000, reason: String::new(), resume_id: "x".into() };
        assert_eq!(
            coordinator_notice(&s.runs["r1"], &wait, 0),
            "[panorama] Coordinator resume for run r1. Since 01:00: t1 completed. 1 unread messages: run check."
        );
    }

    const QUOTA_TAIL: &str = "You've hit your session limit \u{b7} resets 3am (America/Sao_Paulo)";

    fn coordinator_state(run_id: &str, tile: &str) -> State {
        let mut s = new_state(run_id, Autonomy::default());
        f12_commit(&mut s, Cmd::RunSetCoordinator { run_id: run_id.into(), tile_id: Some(tile.into()), now: 2 });
        s
    }

    fn probe(live: bool, idle: bool, tail: &str) -> CoordinatorProbe {
        CoordinatorProbe { live, idle, tail: tail_lines(tail) }
    }

    #[test]
    fn coordinator_step_sets_wait_only_when_idle_with_a_live_quota_tail() {
        let s = coordinator_state("r1", "ct");
        let entry = &s.runs["r1"];
        let set = coordinator_step(entry, NOW, &probe(true, true, QUOTA_TAIL));
        assert!(matches!(set, CoordinatorStep::SetWait { reset_at: Some(at), .. } if at > NOW), "{set:?}");
        assert_eq!(coordinator_step(entry, NOW, &probe(true, false, QUOTA_TAIL)), CoordinatorStep::Nothing);
        assert_eq!(coordinator_step(entry, NOW, &probe(false, true, QUOTA_TAIL)), CoordinatorStep::Nothing);
        assert_eq!(coordinator_step(entry, NOW, &probe(true, true, "all good\n")), CoordinatorStep::Nothing);
    }

    #[test]
    fn coordinator_step_waits_for_reset_plus_grace_and_a_live_tile() {
        let mut s = coordinator_state("r1", "ct");
        f12_commit(&mut s, Cmd::RunSetCoordinatorWait { run_id: "r1".into(), reset_at: Some(NOW), reason: "q".into(), now: 5 });
        let entry = &s.runs["r1"];
        assert_eq!(coordinator_step(entry, NOW + 59_000, &probe(true, true, "")), CoordinatorStep::Nothing);
        assert_eq!(coordinator_step(entry, NOW + 60_000, &probe(false, false, "")), CoordinatorStep::Nothing);
        assert!(matches!(coordinator_step(entry, NOW + 60_000, &probe(true, true, "")), CoordinatorStep::Resume { .. }));
        let mut unknown = coordinator_state("r2", "ct");
        f12_commit(&mut unknown, Cmd::RunSetCoordinatorWait { run_id: "r2".into(), reset_at: None, reason: "q".into(), now: 5 });
        assert_eq!(coordinator_step(&unknown.runs["r2"], NOW * 10, &probe(true, true, "")), CoordinatorStep::Nothing);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn coordinator_wait_is_set_from_a_quota_tail_and_typed_once_after_reset() {
        f12_test_home();
        let run_id = unique("coord");
        let tile = format!("tile-{run_id}");
        let mut s = coordinator_state(&run_id, &tile);
        add_task(&mut s, &run_id, "t1", &["codex:f12c2"], &[]);
        install(s);
        let now = unix_ms() as i64;
        apply_coordinator_step(&run_id, now, &probe(true, true, QUOTA_TAIL)).await;
        let reset_at = {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            guard.runs[&run_id].run.coordinator_wait.as_ref().and_then(|w| w.reset_at).expect("wait set from the tail")
        };
        assert!(reset_at > now);
        let typed = || TEST_TYPED.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(t, _)| *t == tile).count();
        apply_coordinator_step(&run_id, reset_at + 30_000, &probe(true, true, "")).await;
        assert_eq!(typed(), 0);
        apply_coordinator_step(&run_id, reset_at + 61_000, &probe(true, true, "")).await;
        apply_coordinator_step(&run_id, reset_at + 62_000, &probe(true, true, "")).await;
        assert_eq!(typed(), 1);
        let text = TEST_TYPED.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(t, _)| *t == tile).map(|(_, x)| x.clone()).unwrap();
        assert!(text.starts_with(&format!("[panorama] Coordinator resume for run {run_id}. Since ")), "{text}");
        assert!(text.ends_with("unread messages: run check."), "{text}");
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        assert!(guard.runs[&run_id].run.coordinator_wait.is_none());
    }

    fn live_tile(tile: &str) {
        TEST_LIVE_TILES.lock().unwrap_or_else(|e| e.into_inner()).push(tile.to_string());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_retries_an_operationally_failed_task_once() {
        f12_test_home();
        let run_id = unique("retry");
        install(failed_state(&run_id, autonomy(true, false, false), FailureClass::QuotaExhausted));
        let now = unix_ms() as i64;
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        assert_eq!(dispatch_count(&run_id), 2);
        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let d = &guard.runs[&run_id].dispatches[&format!("{run_id}.t1.2")];
            assert_eq!((d.by, d.trigger), (Some(DispatchBy::Brain), Some(Trigger::Fallback)));
            let t = &guard.runs[&run_id].tasks["t1"];
            assert_eq!((t.quality_attempts, t.quality_floor), (0, 0));
        }
        let actions = messages_of(&run_id, "auto_action");
        assert_eq!(actions.len(), 1);
        let body: Value = serde_json::from_str(actions[0].body.as_deref().unwrap()).unwrap();
        assert_eq!((body["action"].as_str(), body["taskId"].as_str()), (Some("retry"), Some("t1")));
        assert_eq!(body["dispatchId"].as_str(), Some(format!("{run_id}.t1.2").as_str()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_resumes_a_due_waiting_task_once() {
        f12_test_home();
        let run_id = unique("resume");
        let mut s = failed_state(&run_id, autonomy(true, false, false), FailureClass::QuotaExhausted);
        let now = unix_ms() as i64;
        wait(&mut s, &run_id, "t1", Some(now - 5_000), false);
        install(s);
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        assert_eq!(dispatch_count(&run_id), 2);
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let t = &guard.runs[&run_id].tasks["t1"];
        assert_eq!((t.status.clone(), t.wait.is_none()), (TaskStatus::Dispatched, true));
        let d = &guard.runs[&run_id].dispatches[&format!("{run_id}.t1.2")];
        assert_eq!((d.by, d.trigger), (Some(DispatchBy::Brain), Some(Trigger::Resume)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_resume_of_a_never_started_waiting_task_records_resume() {
        f12_test_home();
        let run_id = unique("resume0");
        let mut s = new_state(&run_id, autonomy(true, false, false));
        add_task(&mut s, &run_id, "t1", &["codex:f12c2"], &[]);
        let now = unix_ms() as i64;
        wait(&mut s, &run_id, "t1", Some(now - 5_000), false);
        install(s);
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        assert_eq!(dispatch_count(&run_id), 1);
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let d = &guard.runs[&run_id].dispatches[&format!("{run_id}.t1.1")];
        assert_eq!(d.trigger, Some(Trigger::Resume));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_same_session_resume_types_continue_without_a_new_dispatch() {
        f12_test_home();
        let run_id = unique("same");
        let tile = format!("tile-{run_id}");
        live_tile(&tile);
        let mut s = new_state(&run_id, autonomy(true, false, false));
        add_task(&mut s, &run_id, "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut s, &run_id, "t1", Some(&tile));
        fail(&mut s, &run_id, &d, FailureClass::QuotaExhausted);
        let now = unix_ms() as i64;
        wait(&mut s, &run_id, "t1", Some(now - 5_000), true);
        install(s);
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        assert_eq!(dispatch_count(&run_id), 1);
        let typed: Vec<_> = TEST_TYPED.lock().unwrap_or_else(|e| e.into_inner()).iter().filter(|(t, _)| *t == tile).cloned().collect();
        assert_eq!(typed, vec![(tile.clone(), "continue".to_string())]);
        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let entry = &guard.runs[&run_id];
            assert_eq!(entry.dispatches[&d].status, DispatchStatus::Running);
            assert_eq!(entry.tasks["t1"].status, TaskStatus::Dispatched);
        }
        let actions = messages_of(&run_id, "auto_action");
        assert_eq!(actions.len(), 1);
        assert!(actions[0].body.as_deref().unwrap().contains("\"continue\""));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_without_operational_autonomy_sends_resume_ready_once() {
        f12_test_home();
        let run_id = unique("ready");
        let mut s = failed_state(&run_id, Autonomy::default(), FailureClass::QuotaExhausted);
        let now = unix_ms() as i64;
        wait(&mut s, &run_id, "t1", Some(now - 5_000), false);
        install(s);
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        assert_eq!(messages_of(&run_id, "resume_ready").len(), 1);
        assert_eq!(dispatch_count(&run_id), 1);
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(guard.runs[&run_id].tasks["t1"].status, TaskStatus::Waiting);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_execution_autonomy_starts_a_ready_task_once() {
        f12_test_home();
        let run_id = unique("exec");
        let mut s = new_state(&run_id, autonomy(false, true, false));
        add_task(&mut s, &run_id, "t1", &["codex:f12c2"], &[]);
        install(s);
        let now = unix_ms() as i64;
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        assert_eq!(dispatch_count(&run_id), 1);
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let d = &guard.runs[&run_id].dispatches[&format!("{run_id}.t1.1")];
        assert_eq!((d.by, d.trigger), (Some(DispatchBy::Brain), Some(Trigger::Initial)));
    }

    fn git(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(["-C", dir.to_str().unwrap()])
            .args(["-c", "user.name=Test", "-c", "user.email=test@test.local"])
            .args(args)
            .output()
            .expect("git");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    fn feature_branch(repo: &std::path::Path, base: &str, branch: &str, content: &str) {
        git(repo, &["checkout", "-q", "-b", branch, base]);
        std::fs::write(repo.join("shared.txt"), content).unwrap();
        git(repo, &["add", "shared.txt"]);
        git(repo, &["commit", "-q", "-m", branch]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_integration_autonomy_integrates_then_stops_on_conflict() {
        f12_test_home();
        let run_id = unique("int");
        let repo = std::env::temp_dir().join(format!("panorama-c2-{run_id}"));
        std::fs::create_dir_all(&repo).unwrap();
        git(&repo, &["init", "-q"]);
        std::fs::write(repo.join("shared.txt"), "base\n").unwrap();
        git(&repo, &["add", "shared.txt"]);
        git(&repo, &["commit", "-q", "-m", "base"]);
        let base = git(&repo, &["rev-parse", "HEAD"]);
        feature_branch(&repo, &base, "feat-a", "from a\n");
        feature_branch(&repo, &base, "feat-b", "from b\n");

        let mut s = new_state(&run_id, autonomy(false, false, true));
        add_task(&mut s, &run_id, "t1", &["codex:f12c2"], &[]);
        add_task(&mut s, &run_id, "t2", &["codex:f12c2"], &[]);
        let entry = s.runs.get_mut(&run_id).unwrap();
        entry.run.repo = repo.to_string_lossy().into_owned();
        entry.run.base_sha = Some(base);
        install(s);
        for (task, branch) in [("t1", "feat-a"), ("t2", "feat-b")] {
            let mut guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let entry = guard.runs.get_mut(&run_id).unwrap();
            let id = format!("{run_id}.{task}.1");
            let mut d = super::super::tests::mk_dispatch_entry(&id, task, 1, None, None, None, 0);
            d.branch = Some(branch.into());
            d.outcome = Some("succeeded".into());
            entry.dispatches.insert(id, d);
            let t = entry.tasks.get_mut(task).unwrap();
            t.status = TaskStatus::Completed;
            t.attempts = 1;
        }

        let now = unix_ms() as i64;
        tick_run(&run_id, now).await;
        tick_run(&run_id, now + 20_000).await;
        {
            let guard = state().lock().unwrap_or_else(|e| e.into_inner());
            let entry = &guard.runs[&run_id];
            assert!(entry.tasks["t1"].integrated_sha.is_some(), "t1 integrates");
            assert_eq!(entry.tasks["t2"].status, TaskStatus::Blocked, "t2 conflicts and stops alone");
            assert!(entry.tasks["t2"].integrated_sha.is_none());
        }
        assert_eq!(messages_of(&run_id, "auto_action").len(), 1);
        let integration = super::super::git::integration_worktree_path(repo.to_str().unwrap(), &run_id);
        assert!(std::path::Path::new(&integration).exists());
        let _ = std::fs::remove_dir_all(&repo);
    }
}
