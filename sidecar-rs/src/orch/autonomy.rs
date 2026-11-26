use serde_json::{json, Value};
use crate::orch::model::*;
use crate::orch::reducer::{Cmd, RunEntry, WakeReason};
use crate::orch::wiring;
use super::{
    commit, handle_conflict_resolve, handle_integrate, handle_retry, handle_worker_start, send_coordinator_notice, state,
    supervision_watch, tail_lines, unix_ms, CommitResult,
};

const COORDINATOR_RESUME_GRACE_MS: i64 = 60_000;
const NOTICE_TASK_LIMIT: usize = 10;
const SCREEN_TAIL_LINES: usize = 40;
const CONTINUE_TEXT: &str = "continue";
pub(super) const STALL_REASON: &str = "worker stalled";

#[derive(Clone, Debug, PartialEq)]
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
    ConflictResolve { task_id: String },
    ContinueStalled { task_id: String, worker: Revive },
    StopStalled { task_id: String, worker: Revive },
    BudgetExceeded { task_id: String },
}

pub(super) struct TickInput<'a> {
    pub entry: &'a RunEntry,
    pub now: i64,
    pub is_live: &'a dyn Fn(&str) -> bool,
    pub locked_exclusive: &'a [String],
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
    let TickInput { entry, now, is_live, locked_exclusive } = input;
    if entry.run.status != RunStatus::Active {
        return Vec::new();
    }
    let mut slots = (entry.run.limits.max_workers as usize).saturating_sub(entry.running_dispatch_count());
    let mut tasks: Vec<&Task> = entry.tasks.values().collect();
    tasks.sort_by_key(|t| (t.id.len(), t.id.clone()));
    let mut actions = Vec::new();
    let mut locked: Vec<String> = locked_exclusive.to_vec();
    for task in tasks {
        if task.owner.as_deref() == Some("user")
            || (entry.run.detached && entry.auto_action_failures.contains_key(&task.id))
        {
            continue;
        }
        let starts = matches!(task.status, TaskStatus::Waiting | TaskStatus::Failed | TaskStatus::Pending);
        if starts && task_lock_names(task).any(|n| locked.iter().any(|l| l == n)) {
            continue;
        }
        let ctx = TaskCtx { entry, task, now, is_live };
        let planned = match task.status {
            TaskStatus::Waiting => plan_waiting(&ctx, &mut slots),
            TaskStatus::Failed => plan_failed(&ctx, &mut slots),
            TaskStatus::Pending => plan_pending(&ctx, &mut slots),
            TaskStatus::Completed => plan_integrate(&ctx),
            TaskStatus::Blocked if can_resolve_conflict(entry, task) && take_slot(&mut slots) => {
                Some(Action::ConflictResolve { task_id: task.id.clone() })
            }
            TaskStatus::Dispatched => plan_stalled(&ctx),
            _ => None,
        };
        if starts && planned.is_some() {
            locked.extend(task_lock_names(task).map(str::to_string));
        }
        actions.extend(planned);
    }
    actions
}

fn has_counter(task: &Task, counter: FailureCounter) -> bool {
    task.last_failure.is_some_and(|class| class.counter() == counter)
}

fn task_dispatch_cost(entry: &RunEntry, task_id: &str) -> f64 {
    entry.dispatches.values()
        .filter(|d| d.task_id == task_id)
        .map(|d| d.cost_usd.unwrap_or(0.0))
        .sum()
}

fn budget_exceeded(entry: &RunEntry, task: &Task) -> bool {
    task.max_cost_usd.is_some_and(|cap| task_dispatch_cost(entry, &task.id) >= cap)
}

fn budget_exceeded_already_sent(entry: &RunEntry, task_id: &str) -> bool {
    entry.messages.iter().any(|m| m.msg_type == "budget_exceeded" && m.task_id.as_deref() == Some(task_id))
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

fn task_lock_names(task: &Task) -> impl Iterator<Item = &str> {
    let in_place = task.workspace.as_deref() == Some("main");
    task.exclusive.iter().map(String::as_str).chain(in_place.then_some("main"))
}

fn deps_integrated(entry: &RunEntry, task: &Task) -> bool {
    task.deps.iter().all(|dep| {
        entry.tasks.get(dep).is_some_and(|d| {
            d.status == TaskStatus::Completed && (d.integrated_sha.is_some() || d.applied_in_place)
        })
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
    let stalled = entry.latest_dispatch_for_task(&task.id).is_some_and(|d| {
        d.status == DispatchStatus::Stopped && d.summary.as_deref() == Some(STALL_REASON)
    });
    if (task.routing != 1 && !stalled) || !failure_recorded(ctx) {
        return None;
    }
    if budget_exceeded(entry, task) {
        if !budget_exceeded_already_sent(entry, &task.id) {
            return Some(Action::BudgetExceeded { task_id: task.id.clone() });
        }
        return None;
    }
    let autonomy = entry.run.autonomy;
    let limits = &entry.run.limits;
    if autonomy.operational
        && has_counter(task, FailureCounter::Operational)
        && task.operational_attempts < limits.max_operational_attempts
    {
        take_slot(slots);
        let reason = if stalled { STALL_REASON } else { "operational failure, router picks the next candidate" };
        return Some(Action::Retry { task_id: task.id.clone(), stop_live: true, reason });
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
    if task.applied_in_place {
        return None;
    }
    if !entry.run.autonomy.integration || (task.review && !entry.run.detached) || task.integrated_sha.is_some() {
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

pub(super) fn can_resolve_conflict(entry: &RunEntry, task: &Task) -> bool {
    entry.run.status == RunStatus::Active
        && entry.run.autonomy.integration
        && task.status == TaskStatus::Blocked
        && task.owner.as_deref() != Some("user")
        && !has_open_gate(entry, task)
        && entry.integration_conflicts.contains(&task.id)
        && !(entry.run.detached && entry.auto_action_failures.contains_key(&task.id))
        && entry.dispatches.values().filter(|d| d.task_id == task.id && d.merge_branch.is_some()).count() < 2
}

pub(super) fn can_handle_stall(entry: &RunEntry, task: &Task, dispatch: &Dispatch) -> bool {
    entry.run.status == RunStatus::Active
        && entry.run.autonomy.operational
        && task.status == TaskStatus::Dispatched
        && task.owner.as_deref() != Some("user")
        && !(entry.run.detached && entry.auto_action_failures.contains_key(&task.id))
        && dispatch.status == DispatchStatus::Running
}

pub(super) fn stall_continuation<'a>(entry: &'a RunEntry, dispatch_id: &str) -> Option<&'a Message> {
    entry.messages.iter().rev().find(|m| {
        m.from == "brain" && m.msg_type == "auto_action" && m.dispatch_id.as_deref() == Some(dispatch_id)
            && m.body.as_deref().and_then(|body| serde_json::from_str::<Value>(body).ok())
                .is_some_and(|body| body["action"] == "continue" && body["reason"] == STALL_REASON)
    })
}

pub(super) fn stall_pending(entry: &RunEntry, dispatch_id: &str, continuation: Option<&Message>) -> bool {
    entry.messages.iter().rev().take_while(|m| continuation.is_none_or(|c| m.seq > c.seq))
        .any(|m| m.from == "brain" && m.msg_type == "worker_stalled" && m.dispatch_id.as_deref() == Some(dispatch_id))
}

fn plan_stalled(ctx: &TaskCtx) -> Option<Action> {
    let (entry, task) = (ctx.entry, ctx.task);
    let dispatch = entry.latest_dispatch_for_task(&task.id)?;
    if !can_handle_stall(entry, task, dispatch) {
        return None;
    }
    let continuation = stall_continuation(entry, &dispatch.id);
    if !stall_pending(entry, &dispatch.id, continuation) {
        return None;
    }
    let tile_id = dispatch.tile_id.clone()?;
    let worker = Revive { dispatch_id: dispatch.id.clone(), tile_id };
    let task_id = task.id.clone();
    if continuation.is_some() {
        Some(Action::StopStalled { task_id, worker })
    } else {
        (ctx.is_live)(&worker.tile_id).then_some(Action::ContinueStalled { task_id, worker })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IdleTask {
    pub id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IdleGroups {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting_intervention: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub open_gate: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting_no_resume: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resume_due: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unintegrated: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_action_failed: Vec<IdleTask>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verifying: Vec<IdleTask>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IdleCounts {
    pub completed: usize,
    pub integrated: usize,
    pub cancelled: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct IdleReport {
    pub reason: String,
    pub counts: IdleCounts,
    pub tasks: IdleGroups,
}

fn idle_task(task: &Task, detail: Option<String>) -> IdleTask {
    IdleTask { id: task.id.clone(), title: task.title.clone(), detail }
}

fn open_gate_detail(entry: &RunEntry, task: &Task) -> Option<String> {
    let mut gates: Vec<&Gate> = entry.gates.values()
        .filter(|g| g.task_id == task.id && g.resolution.is_none())
        .collect();
    if gates.is_empty() {
        return None;
    }
    gates.sort_by_key(|g| (g.id.len(), g.id.clone()));
    Some(gates.into_iter().map(|g| format!("{}: {}", g.id, g.question)).collect::<Vec<_>>().join("; "))
}

fn failed_detail(entry: &RunEntry, task: &Task) -> Option<String> {
    if let Some(dispatch) = entry.latest_dispatch_for_task(&task.id) {
        if let Some(v) = dispatch.verifications.last()
            .filter(|v| dispatch.outcome.as_deref() == Some("succeeded") && (v.timed_out || v.exit_code != Some(0)))
        {
            return Some(if v.timed_out {
                format!("Verification timed out: {}", v.command)
            } else {
                format!("Verification failed: {}", v.command)
            });
        }
        if let Some(handoff) = entry.failed_worker_handoff(dispatch) {
            return Some(handoff);
        }
        if let Some(line) = dispatch.summary.as_deref()
            .and_then(|s| s.lines().find(|line| !line.trim().is_empty()))
        {
            return Some(line.to_string());
        }
    }
    task.last_failure.map(|class| format!("{class:?}"))
}

pub(super) fn idle_reason(entry: &RunEntry, now: i64) -> Option<IdleReport> {
    if !entry.run.detached || entry.run.status != RunStatus::Active || entry.tasks.is_empty() {
        return None;
    }
    if entry.running_dispatch_count() > 0
        || entry.tasks.values().any(|t| t.status == TaskStatus::Dispatched
            || (t.status == TaskStatus::Verifying && entry.latest_dispatch_for_task(&t.id)
                .is_some_and(|d| super::VerificationFlight::contains(&d.id))))
    {
        return None;
    }
    let actions = plan(TickInput { entry, now, is_live: &|_| false, locked_exclusive: &[] });
    if actions.iter().any(|a| !matches!(a, Action::ResumeReady { .. })) {
        return None;
    }
    let mut tasks: Vec<&Task> = entry.tasks.values().collect();
    tasks.sort_by_key(|t| (t.id.len(), t.id.clone()));
    let mut counts = IdleCounts::default();
    let mut groups = IdleGroups::default();
    for task in tasks {
        match task.status {
            TaskStatus::Completed => {
                counts.completed += 1;
                if task.integrated_sha.is_some() || task.applied_in_place {
                    counts.integrated += 1;
                } else if let Some(failure) = entry.auto_action_failures.get(&task.id) {
                    groups.auto_action_failed.push(idle_task(task, Some(format!("{}: {}", failure.action, failure.error))));
                } else {
                    groups.unintegrated.push(idle_task(task, None));
                }
            }
            TaskStatus::Cancelled => {
                counts.cancelled += 1;
            }
            _ if entry.auto_action_failures.contains_key(&task.id) => {
                let failure = &entry.auto_action_failures[&task.id];
                groups.auto_action_failed.push(idle_task(task, Some(format!("{}: {}", failure.action, failure.error))));
            }
            _ if has_open_gate(entry, task) => {
                groups.open_gate.push(idle_task(task, open_gate_detail(entry, task)));
            }
            TaskStatus::Failed => {
                groups.failed.push(idle_task(task, failed_detail(entry, task)));
            }
            TaskStatus::Blocked => {
                groups.blocked.push(idle_task(task, None));
            }
            TaskStatus::Review => {
                groups.review.push(idle_task(task, None));
            }
            TaskStatus::Pending => {
                let detail = (task.owner.as_deref() == Some("user")).then(|| "owner: user".into());
                groups.pending.push(idle_task(task, detail));
            }
            TaskStatus::Waiting => match task.wait.as_ref() {
                Some(wait) if entry.run.autonomy.operational
                    && task.owner.as_deref() != Some("user")
                    && matches!(wait.kind, WaitKind::Quota | WaitKind::Availability)
                    && wait.resume_at.is_some_and(|at| at > now) => {}
                Some(wait) if wait.kind == WaitKind::Intervention => {
                    groups.waiting_intervention.push(idle_task(task, Some(wait.reason.clone())));
                }
                Some(wait) if matches!(wait.kind, WaitKind::Quota | WaitKind::Availability)
                    && wait.resume_at.is_some_and(|at| at <= now) => {
                    groups.resume_due.push(idle_task(task, Some(wait.reason.clone())));
                }
                wait => {
                    groups.waiting_no_resume.push(idle_task(task, wait.map(|w| w.reason.clone())));
                }
            },
            TaskStatus::Verifying => {
                groups.verifying.push(idle_task(task, Some("verification is not running".into())));
            }
            TaskStatus::Dispatched => return None,
        }
    }
    let finished = counts.completed == counts.integrated
        && counts.completed + counts.cancelled == entry.tasks.len();
    if !finished && groups == IdleGroups::default() {
        return None;
    }
    Some(IdleReport {
        reason: if finished { "finished" } else { "idle" }.into(),
        counts,
        tasks: groups,
    })
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
    Resume { resume_id: String, tile_id: String, notice: String, unread: usize, detached: bool },
}

fn status_word(status: &TaskStatus) -> String {
    serde_json::to_value(status).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn clock_label(at_ms: i64, offset_min: i32) -> String {
    let minutes = (at_ms / 60_000 + offset_min as i64).rem_euclid(24 * 60);
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

fn coordinator_notice(entry: &RunEntry, wait: &CoordinatorWait, offset_min: i32) -> (String, usize) {
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
    let notice = format!(
        "[panorama] Coordinator resume for run {}. Since {}: {}. {} unread messages: run check.",
        entry.run.id,
        clock_label(wait.since, offset_min),
        lines.join("; "),
        unread,
    );
    (notice, unread)
}

pub(super) fn coordinator_step(entry: &RunEntry, now: i64, probe: &CoordinatorProbe) -> CoordinatorStep {
    let Some(tile_id) = entry.run.coordinator_tile.clone().filter(|_| probe.live) else {
        return CoordinatorStep::Nothing;
    };
    if let Some(wait) = &entry.run.coordinator_wait {
        return match wait.reset_at {
            Some(at) if now >= at + COORDINATOR_RESUME_GRACE_MS => {
                let (notice, unread) = coordinator_notice(entry, wait, wiring::local_offset_min());
                CoordinatorStep::Resume {
                    resume_id: wait.resume_id.clone(), tile_id, notice, unread, detached: entry.run.detached,
                }
            }
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

pub(super) fn locked_exclusive_names_canon(path: &str) -> String {
    std::fs::canonicalize(path)
        .map(|p| {
            let s = p.to_string_lossy();
            let trimmed = s.trim_start_matches(r"\\?\");
            trimmed.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase()
        })
        .unwrap_or_else(|_| path.replace('\\', "/").trim_end_matches('/').to_ascii_lowercase())
}

pub(super) fn locked_exclusive_names(state: &crate::orch::reducer::State, repo: &str) -> Vec<String> {
    let repo_canon = locked_exclusive_names_canon(repo);
    let mut names: Vec<String> = Vec::new();
    for entry in state.runs.values() {
        if locked_exclusive_names_canon(&entry.run.repo) != repo_canon {
            continue;
        }
        for task in entry.tasks.values() {
            if !matches!(task.status, TaskStatus::Dispatched | TaskStatus::Verifying) {
                continue;
            }
            for name in task_lock_names(task) {
                if !names.iter().any(|n| n == name) {
                    names.push(name.to_string());
                }
            }
        }
    }
    names
}

pub(super) async fn tick_run(run_id: &str, now: i64) {
    let actions = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.runs.get(run_id) else { return };
        let locked = locked_exclusive_names(&guard, &entry.run.repo);
        plan(TickInput { entry, now, is_live: &tile_is_live, locked_exclusive: &locked })
    };
    for action in actions {
        execute(run_id, action).await;
    }
    coordinator_tick(run_id, now).await;
    super::sync_idle(run_id).await;
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

fn action_target(run_id: &str, task_id: &str) -> Option<(TaskStatus, u64)> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    let entry = guard.runs.get(run_id).filter(|entry| entry.run.detached)?;
    let task = entry.tasks.get(task_id)?;
    Some((task.status.clone(), task.updated_at))
}

pub(super) async fn clear_action_failure(run_id: &str, task_id: &str) {
    let recorded = state().lock().unwrap_or_else(|e| e.into_inner()).runs.get(run_id)
        .is_some_and(|entry| entry.auto_action_failures.contains_key(task_id));
    if recorded {
        let (run, task) = (run_id.to_string(), task_id.to_string());
        super::commit_deferred_idle(move |_| Ok(Cmd::TaskAutoActionFailure {
            run_id: run, task_id: task, failure: None, expected: None, now: unix_ms(),
        }), run_id.to_string()).await;
    }
}

async fn record_action_failure(run_id: &str, task_id: &str, action: (&'static str, String), target: Option<(TaskStatus, u64)>) {
    skipped(run_id, task_id, action.0, &action.1);
    let Some(target) = target else { return };
    let (run, task) = (run_id.to_string(), task_id.to_string());
    commit(move |_| Ok(Cmd::TaskAutoActionFailure {
        run_id: run, task_id: task, failure: Some(action), expected: Some(target), now: unix_ms(),
    }), run_id.to_string()).await;
}

fn response_error(response: &Value, raw: &str) -> String {
    response["error"]["message"].as_str().unwrap_or(raw).to_string()
}

async fn finish_dispatching(run_id: &str, task_id: &str, what: (&'static str, &str), result: (&str, Option<(TaskStatus, u64)>)) -> bool {
    let (resp, target) = result;
    let v = serde_json::from_str::<Value>(resp).unwrap_or_default();
    if v["ok"].as_bool() != Some(true) {
        record_action_failure(run_id, task_id, (what.0, response_error(&v, resp)), target).await;
        return false;
    }
    if v.get("waiting").is_some() {
        return false;
    }
    let dispatch_id = v["dispatch"]["id"].as_str().map(str::to_string);
    announce(run_id, task_id, dispatch_id, what).await;
    true
}

fn latest_dispatch_id(run_id: &str, task_id: &str) -> Option<String> {
    let guard = state().lock().unwrap_or_else(|e| e.into_inner());
    guard.runs.get(run_id)?.latest_dispatch_for_task(task_id).map(|d| d.id.clone())
}

async fn execute(run_id: &str, action: Action) {
    match action {
        Action::Retry { task_id, stop_live, reason } => {
            retry_task(run_id, &task_id, stop_live, reason).await;
        }
        Action::Start { task_id, retry_of, stop_live, reason } => {
            let target = action_target(run_id, &task_id);
            let resp = handle_worker_start(&request_body(run_id, &task_id, json!({"stopLive": stop_live, "retryOf": retry_of}))).await;
            finish_dispatching(run_id, &task_id, ("start", reason), (&resp, target)).await;
        }
        Action::Integrate { task_id } => {
            let target = action_target(run_id, &task_id);
            let resp = handle_integrate(&request_body(run_id, &task_id, json!({}))).await;
            let response = serde_json::from_str::<Value>(&resp).unwrap_or_default();
            if response["integrated"] == true {
                announce(run_id, &task_id, None, ("integrate", "completed task merged into the integration branch")).await;
            } else if response["conflict"] != true {
                record_action_failure(run_id, &task_id, ("integrate", response_error(&response, &resp)), target).await;
            }
        }
        Action::ConflictResolve { task_id } => {
            let target = action_target(run_id, &task_id);
            let resp = handle_conflict_resolve(&request_body(run_id, &task_id, json!({}))).await;
            finish_dispatching(run_id, &task_id, ("conflict_resolve", "integration conflict"), (&resp, target)).await;
        }
        Action::ContinueStalled { task_id, worker } => {
            let target = action_target(run_id, &task_id);
            let current = {
                let guard = state().lock().unwrap_or_else(|e| e.into_inner());
                guard.runs.get(run_id).and_then(|entry| entry.tasks.get(&task_id).and_then(|task| {
                    plan_stalled(&TaskCtx { entry, task, now: unix_ms() as i64, is_live: &tile_is_live })
                }))
            };
            if current != Some(Action::ContinueStalled { task_id: task_id.clone(), worker: worker.clone() }) {
                return;
            }
            if let Err(error) = type_continue(&worker) {
                record_action_failure(run_id, &task_id, ("continue", error), target).await;
            } else {
                announce(run_id, &task_id, Some(worker.dispatch_id), ("continue", STALL_REASON)).await;
            }
        }
        Action::StopStalled { task_id, worker } => stop_stalled(run_id, &task_id, worker).await,
        Action::Resume { task_id, resume_id, revive } => resume(run_id, &task_id, &resume_id, revive).await,
        Action::ResumeReady { task_id, dispatch_id, body } => {
            send_coordinator_notice(run_id.to_string(), task_id, dispatch_id, "resume_ready", body).await;
        }
        Action::BudgetExceeded { task_id } => {
            let dispatch_id = latest_dispatch_id(run_id, &task_id);
            let body = json!({"taskId": task_id}).to_string();
            send_coordinator_notice(run_id.to_string(), task_id, dispatch_id, "budget_exceeded", body).await;
        }
    }
}

async fn stop_stalled(run_id: &str, task_id: &str, worker: Revive) {
    let (run, dispatch) = (run_id.to_string(), worker.dispatch_id.clone());
    let stopped = super::commit_deferred_idle(move |g| {
        let entry = g.runs.get(&run).ok_or("not_found:run not found")?;
        let d = entry.dispatches.get(&dispatch).ok_or("not_found:dispatch not found")?;
        let task = &entry.tasks[&d.task_id];
        if !can_handle_stall(entry, task, d)
            || !entry.latest_dispatch_for_task(&task.id).is_some_and(|latest| latest.id == dispatch)
            || !stall_continuation(entry, &dispatch).is_some_and(|c| stall_pending(entry, &dispatch, Some(c)))
        {
            return Err("conflict:stalled dispatch changed".into());
        }
        Ok(Cmd::DispatchUpdate {
            run_id: run, dispatch_id: dispatch, status: DispatchStatus::Stopped,
            summary: Some(STALL_REASON.into()), now: unix_ms(),
        }.with_fields(json!({"routedFailureClass": FailureClass::ExecutionFailed})))
    }, run_id.to_string()).await;
    if !matches!(stopped, CommitResult::Ok(_)) {
        return;
    }
    super::stop_worker_session(run_id, &worker.dispatch_id, &worker.tile_id);
    let retry = {
        let guard = state().lock().unwrap_or_else(|e| e.into_inner());
        guard.runs.get(run_id).and_then(|entry| {
            let task = entry.tasks.get(task_id)?;
            if entry.run.status != RunStatus::Active || task.status != TaskStatus::Failed || task.owner.as_deref() == Some("user") {
                return None;
            }
            plan_failed(&TaskCtx { entry, task, now: unix_ms() as i64, is_live: &tile_is_live }, &mut 0)
        })
    };
    if let Some(Action::Retry { stop_live, reason, .. }) = retry {
        if retry_task(run_id, task_id, stop_live, reason).await {
            return;
        }
    }
    announce(run_id, task_id, Some(worker.dispatch_id), ("stop", STALL_REASON)).await;
}

async fn retry_task(run_id: &str, task_id: &str, stop_live: bool, reason: &'static str) -> bool {
    let target = action_target(run_id, task_id);
    let resp = handle_retry(&request_body(run_id, task_id, json!({"stopLive": stop_live}))).await;
    finish_dispatching(run_id, task_id, ("retry", reason), (&resp, target)).await
}

fn type_continue(worker: &Revive) -> Result<(), String> {
    supervision_watch().lock().unwrap_or_else(|e| e.into_inner()).remove(&worker.dispatch_id);
    type_into_tile(&worker.tile_id, CONTINUE_TEXT)
}

async fn revive_dispatch(run_id: &str, task_id: &str, continuation: (&Revive, &str), target: Option<(TaskStatus, u64)>) -> bool {
    let (revive, reason) = continuation;
    let (run_c, dispatch_c) = (run_id.to_string(), revive.dispatch_id.clone());
    let now = unix_ms();
    let revived = commit(move |_| Ok(Cmd::DispatchRevive { run_id: run_c, dispatch_id: dispatch_c, now }), run_id.to_string()).await;
    match revived {
        CommitResult::Ok(_) => {}
        CommitResult::Err(error) if target.is_some() => {
            record_action_failure(run_id, task_id, ("continue", error), target).await;
            return true;
        }
        _ => return false,
    }
    if let Err(error) = type_continue(revive) {
        let revived_target = target.as_ref().map(|_| (TaskStatus::Dispatched, now));
        record_action_failure(run_id, task_id, ("continue", error), revived_target).await;
        if target.is_some() {
            return true;
        }
    }
    announce(run_id, task_id, Some(revive.dispatch_id.clone()), ("continue", reason)).await;
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
    let target = action_target(run_id, task_id);
    if let Some(revive) = revive {
        if revive_dispatch(run_id, task_id, (&revive, &reason), target.clone()).await {
            return;
        }
    }
    let retry_of = latest_dispatch_id(run_id, task_id);
    let resp = handle_worker_start(&request_body(run_id, task_id, json!({"stopLive": true, "retryOf": retry_of, "resume": true}))).await;
    finish_dispatching(run_id, task_id, ("start", &reason), (&resp, target)).await;
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
        CoordinatorStep::Resume { resume_id, tile_id, notice, unread, detached } => {
            let run_c = run_id.to_string();
            let cleared = commit(move |_| Ok(Cmd::RunClearCoordinatorWait { run_id: run_c, resume_id, now: unix_ms() }), run_id.to_string()).await;
            if !matches!(cleared, CommitResult::Ok(_)) {
                return;
            }
            let sent = if crate::agents::is_agent_tile(&tile_id) {
                crate::agents::orch_notice(&tile_id, run_id, &notice)
            } else {
                type_into_tile(&tile_id, &notice)
            };
            match sent {
                Ok(()) => super::record_coordinator_wake(
                    run_id, WakeReason::CoordinatorResume, unread, detached,
                ),
                Err(e) => skipped(run_id, "coordinator", "resume-notice", &e),
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
            verify: vec![], exclusive: vec![], role: None, review: false, owner: None, now: 1_000_000,
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
        plan(TickInput { entry: &s.runs[run_id], now, is_live: &|_| true, locked_exclusive: &[] })
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
        s.runs.get_mut("r1").unwrap().run.detached = true;
        assert!(matches!(plan_at(&s, "r1", NOW).as_slice(), [Action::Integrate { .. }]));
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Review;
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
            verify: vec![], exclusive: vec![], role: None, review: false, owner: None, now: 1_000_000,
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
            spec: None, acceptance: None, verify: None, profile: None, escalation: None, exclusive: None, review: None, now: 5,
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
            ("[panorama] Coordinator resume for run r1. Since 01:00: t1 completed. 1 unread messages: run check.".into(), 1),
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
        let entry = &guard.runs[&run_id];
        assert!(entry.run.coordinator_wait.is_none());
        assert_eq!(entry.coordinator_wakes.len(), 1);
        assert_eq!(serde_json::to_value(&entry.coordinator_wakes[0].reason).unwrap(), "coordinator_resume");
        assert!(!entry.coordinator_wakes[0].detached);
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

    fn persist(run_id: &str, cmd: Cmd) {
        let home = crate::panorama_dir().unwrap();
        assert!(matches!(super::super::commit_sync_raw(|_| Ok(cmd), run_id, &home), CommitResult::Ok(_)));
    }

    fn persisted_run(run_id: &str, repo: &str, base_sha: Option<String>, auto: Autonomy, limits: Limits) {
        persist(run_id, Cmd::RunCreate {
            id: run_id.into(), title: "test".into(), repo: repo.into(), base_ref: None, base_sha,
            setup: None, coordinator_tile: None, limits, now: 1,
        }.with_fields(json!({"autonomy": auto, "detached": true})));
    }

    fn persisted_task(run_id: &str, task_id: &str, tile_id: Option<String>, routing: u32) {
        persist(run_id, Cmd::TaskCreate {
            run_id: run_id.into(), id: task_id.into(), title: task_id.into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec!["codex:f12c2".into()], verify: vec![], exclusive: vec![],
            role: None, review: false, owner: None, now: 2,
        }.with_fields(json!({"routing": routing})));
        persist(run_id, Cmd::Dispatch {
            run_id: run_id.into(), id: format!("{run_id}.{task_id}.1"), task_id: task_id.into(), attempt: 1,
            retry_of: None, profile: Some("codex".into()), model: Some("f12c2".into()), tile_id,
            owner: None, merge_branch: None, now: 3,
        });
    }

    fn replayed(run_id: &str) -> State {
        let base = crate::panorama_dir().unwrap();
        let mut replay = State::default();
        super::super::load_into(&mut replay, &super::super::run_events_path(&base, run_id), run_id);
        replay
    }

    fn auto_action_names(run_id: &str) -> Vec<String> {
        messages_of(run_id, "auto_action").iter().map(|m| {
            serde_json::from_str::<Value>(m.body.as_deref().unwrap()).unwrap()["action"].as_str().unwrap().to_string()
        }).collect()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tick_integration_autonomy_resolves_conflicts_at_most_twice() {
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

        let repo_path = repo.to_str().unwrap();
        persisted_run(&run_id, repo_path, Some(base), autonomy(false, false, true), Limits::default());
        let report_branch = |task: &str, attempt, branch: &str| {
            let id = format!("{run_id}.{task}.{attempt}");
            persist(&run_id, Cmd::DispatchSetWorktree {
                run_id: run_id.clone(), dispatch_id: id.clone(), worktree: repo_path.into(), branch: branch.into(), now: unix_ms(),
            });
            let seq = state().lock().unwrap().runs[&run_id].next_msg_seq();
            persist(&run_id, Cmd::Send {
                run_id: run_id.clone(), seq, from: id.clone(), to: vec!["coordinator".into()],
                msg_type: "worker_done".into(), task_id: Some(task.into()), dispatch_id: Some(id),
                reply_to: None, body: None, outcome: Some("succeeded".into()), files_modified: vec![],
                summary: None, now: unix_ms(),
            });
        };
        persisted_task(&run_id, "t1", None, 1);
        persisted_task(&run_id, "t2", None, 1);
        report_branch("t1", 1, "feat-a");
        report_branch("t2", 1, "feat-b");

        let now = unix_ms() as i64;
        for attempt in 1..=3 {
            tick_run(&run_id, now).await;
            let mut replay = replayed(&run_id);
            let entry = &replay.runs[&run_id];
            assert!(entry.tasks["t1"].integrated_sha.is_some(), "t1 integrates");
            assert_eq!(entry.tasks["t2"].status, TaskStatus::Blocked);
            assert!(entry.auto_action_failures.is_empty());
            let expected = if attempt < 3 { vec![Action::ConflictResolve { task_id: "t2".into() }] } else { vec![] };
            assert_eq!(plan_at(&replay, &run_id, now), expected);
            {
                let guard = state().lock().unwrap();
                let entry = &guard.runs[&run_id];
                assert!(entry.auto_action_failures.is_empty());
                assert_eq!(plan(TickInput { entry, now, is_live: &|_| true, locked_exclusive: &[] }), expected);
                let conflict = entry.messages.iter().rev().find(|m| m.msg_type == "integration_conflict").unwrap();
                assert_eq!(super::super::needs_decision(conflict, entry, now), attempt == 3);
            }
            replay.runs.get_mut(&run_id).unwrap().run.autonomy.integration = false;
            assert!(plan_at(&replay, &run_id, now).is_empty());
            let entry = replay.runs.get_mut(&run_id).unwrap();
            entry.run.autonomy.integration = true;
            entry.tasks.get_mut("t2").unwrap().owner = Some("user".into());
            assert!(plan_at(&replay, &run_id, now).is_empty());
            if attempt < 3 {
                let entry = replay.runs.get_mut(&run_id).unwrap();
                entry.tasks.get_mut("t2").unwrap().owner = None;
                let gate_seq = entry.next_msg_seq();
                f12_commit(&mut replay, Cmd::GateCreate {
                    run_id: run_id.clone(), id: "g1".into(), task_id: "t2".into(), question: "Proceed?".into(),
                    options: vec![], msg_seq: gate_seq, now: unix_ms(),
                });
                assert!(plan_at(&replay, &run_id, now).is_empty());
                tick_run(&run_id, now).await;
                assert_eq!(dispatch_count(&run_id), attempt + 2);
                let actions = messages_of(&run_id, "auto_action");
                let action = actions.last().unwrap();
                assert_eq!(action.dispatch_id.as_deref(), Some(format!("{run_id}.t2.{}", attempt + 1).as_str()));
                report_branch("t2", attempt + 1, "feat-b");
            }
        }
        tick_run(&run_id, now).await;
        assert_eq!(dispatch_count(&run_id), 4);
        assert_eq!(auto_action_names(&run_id), ["integrate", "conflict_resolve", "conflict_resolve"]);
        let integration = super::super::git::integration_worktree_path(repo_path, &run_id);
        assert!(std::path::Path::new(&integration).exists());
        let _ = std::fs::remove_dir_all(&repo);
    }

    #[tokio::test]
    async fn tick_stalled_dispatch_continues_once_then_retries_within_operational_budget() {
        f12_test_home();
        for (operational, routing, cap) in [(false, 1, 2), (true, 1, 2), (true, 0, 2), (true, 1, 1)] {
            let run_id = unique(&format!("stall-{operational}-{routing}-{cap}"));
            let tile = format!("tile-{run_id}");
            let dispatch_id = format!("{run_id}.t1.1");
            live_tile(&tile);
            persisted_run(&run_id, "/tmp", None, autonomy(operational, false, false),
                Limits { max_operational_attempts: cap, max_attempts: 1, ..Limits::default() });
            persisted_task(&run_id, "t1", Some(tile.clone()), routing);
            let stall = |reason| {
                let seq = state().lock().unwrap().runs[&run_id].next_msg_seq();
                persist(&run_id, Cmd::Send {
                    run_id: run_id.clone(), seq, from: "brain".into(), to: vec!["coordinator".into()],
                    msg_type: "worker_stalled".into(), task_id: Some("t1".into()), dispatch_id: Some(dispatch_id.clone()),
                    reply_to: None, body: Some(json!({"reason": reason}).to_string()), outcome: None,
                    files_modified: vec![], summary: None, now: unix_ms(),
                });
            };
            let now = unix_ms() as i64;
            stall("timeout");
            let replay = replayed(&run_id);
            assert_eq!(matches!(plan_at(&replay, &run_id, now).as_slice(), [Action::ContinueStalled { .. }]), operational);
            tick_run(&run_id, now).await;
            tick_run(&run_id, now).await;
            assert!(plan_at(&replayed(&run_id), &run_id, now).is_empty());
            stall("quiet");
            let replay = replayed(&run_id);
            assert_eq!(matches!(plan_at(&replay, &run_id, now).as_slice(), [Action::StopStalled { .. }]), operational);
            tick_run(&run_id, now).await;
            tick_run(&run_id, now).await;
            let typed = TEST_TYPED.lock().unwrap().iter().filter(|(t, text)| t == &tile && text == "continue").count();
            assert_eq!(typed, usize::from(operational));
            let retried = operational && cap > 1;
            assert_eq!(dispatch_count(&run_id), 1 + usize::from(retried));
            let replay = replayed(&run_id);
            let entry = &replay.runs[&run_id];
            let task = &entry.tasks["t1"];
            assert_eq!(task.operational_attempts, u32::from(operational));
            assert_eq!(task.quality_attempts, 0);
            assert_eq!(entry.dispatches[&dispatch_id].status, if operational { DispatchStatus::Stopped } else { DispatchStatus::Running });
            assert!(plan_at(&replay, &run_id, now).is_empty());
            let actions = auto_action_names(&run_id);
            if operational {
                assert_eq!(actions, ["continue", if retried { "retry" } else { "stop" }]);
            } else {
                assert!(actions.is_empty());
            }
        }
    }

    fn detached_state(auto: Autonomy) -> State {
        let mut s = new_state("r1", auto);
        s.runs.get_mut("r1").unwrap().run.detached = true;
        s
    }

    #[test]
    fn future_wait_does_not_hide_review_idle() {
        let mut s = detached_state(autonomy(true, true, true));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        let now = unix_ms() as i64;
        wait(&mut s, "r1", "t1", Some(now + 60_000), false);
        let base = std::env::temp_dir().join(format!("panorama-idle-wait-{now}"));
        std::fs::create_dir_all(&base).unwrap();
        assert!(!super::super::sync_idle_locked(&mut s, "r1", &base));
        add_task(&mut s, "r1", "t2", &["codex:f12c2"], &[]);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t2").unwrap().status = TaskStatus::Review;
        assert!(super::super::sync_idle_locked(&mut s, "r1", &base));
        let messages = &s.runs["r1"].messages;
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].msg_type, "run_idle");
        assert!(messages[0].body.as_deref().unwrap().contains("t2 (t2): Review"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn due_wait_tick_emits_changed_idle_once() {
        f12_test_home();
        let run_id = unique("due-idle");
        let now = unix_ms() as i64;
        let mut s = failed_state(&run_id, autonomy(false, false, false), FailureClass::QuotaExhausted);
        s.runs.get_mut(&run_id).unwrap().run.detached = true;
        wait(&mut s, &run_id, "t1", Some(now + 60_000), false);
        install(s);
        tick_run(&run_id, now).await;
        assert_eq!(messages_of(&run_id, "run_idle").len(), 1);
        state().lock().unwrap_or_else(|e| e.into_inner()).runs.get_mut(&run_id).unwrap()
            .tasks.get_mut("t1").unwrap().wait.as_mut().unwrap().resume_at = Some(now - 1);
        tick_run(&run_id, now).await;
        tick_run(&run_id, now).await;
        let messages = messages_of(&run_id, "run_idle");
        assert_eq!(messages.len(), 2);
        assert!(messages[1].body.as_deref().unwrap().contains("resume due"));
    }

    #[test]
    fn idle_reason_table_covers_all_reasons() {
        let attached = new_state("r1", autonomy(true, true, true));
        assert!(idle_reason(&attached.runs["r1"], NOW).is_none());

        let empty = detached_state(autonomy(true, true, true));
        assert!(idle_reason(&empty.runs["r1"], NOW).is_none());

        for status in [RunStatus::Paused, RunStatus::Completed] {
            let mut s = detached_state(autonomy(true, true, true));
            add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
            s.runs.get_mut("r1").unwrap().run.status = status;
            assert!(idle_reason(&s.runs["r1"], NOW).is_none());
        }

        let mut running = detached_state(autonomy(true, true, true));
        add_task(&mut running, "r1", "t1", &["codex:f12c2"], &[]);
        dispatch(&mut running, "r1", "t1", None);
        assert!(idle_reason(&running.runs["r1"], NOW).is_none());

        let mut verifying = detached_state(autonomy(true, true, true));
        add_task(&mut verifying, "r1", "t1", &["codex:f12c2"], &[]);
        verifying.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Verifying;
        assert_eq!(idle_reason(&verifying.runs["r1"], NOW).unwrap().tasks.verifying[0].id, "t1");

        let mut legacy_fail = detached_state(autonomy(false, true, false));
        add_task(&mut legacy_fail, "r1", "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut legacy_fail, "r1", "t1", None);
        f12_commit(&mut legacy_fail, Cmd::DispatchUpdate {
            run_id: "r1".into(), dispatch_id: d, status: DispatchStatus::Failed,
            summary: Some("first line\nsecond line".into()), now: 1_000_100,
        });
        legacy_fail.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().routing = 0;
        let r = idle_reason(&legacy_fail.runs["r1"], NOW).expect("legacy failed idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.failed.len(), 1);
        assert_eq!(r.tasks.failed[0].id, "t1");
        assert_eq!(r.tasks.failed[0].detail.as_deref(), Some("first line"));

        let mut op_off = detached_state(autonomy(false, true, true));
        add_task(&mut op_off, "r1", "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut op_off, "r1", "t1", None);
        fail(&mut op_off, "r1", &d, FailureClass::ExecutionFailed);
        let r = idle_reason(&op_off.runs["r1"], NOW).expect("operational off idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.failed.len(), 1);
        assert_eq!(r.tasks.failed[0].detail.as_deref(), Some("ExecutionFailed"));

        let mut op_on = detached_state(autonomy(true, true, true));
        add_task(&mut op_on, "r1", "t1", &["codex:f12c2"], &[]);
        let d = dispatch(&mut op_on, "r1", "t1", None);
        fail(&mut op_on, "r1", &d, FailureClass::ExecutionFailed);
        assert!(idle_reason(&op_on.runs["r1"], NOW).is_none());

        let mut exec_off = detached_state(autonomy(true, false, true));
        add_task(&mut exec_off, "r1", "t1", &["codex:f12c2"], &[]);
        let r = idle_reason(&exec_off.runs["r1"], NOW).expect("execution off idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.pending.len(), 1);
        assert_eq!(r.tasks.pending[0].id, "t1");

        let mut gated = detached_state(autonomy(true, true, true));
        add_task(&mut gated, "r1", "t1", &["codex:f12c2"], &[]);
        gated.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Blocked;
        f12_commit(&mut gated, Cmd::GateCreate {
            run_id: "r1".into(), id: "g1".into(), task_id: "t1".into(),
            question: "Pick a path?".into(), options: vec![], msg_seq: 1, now: 1_000_200,
        });
        let r = idle_reason(&gated.runs["r1"], NOW).expect("open gate idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.open_gate.len(), 1);
        assert_eq!(r.tasks.open_gate[0].detail.as_deref(), Some("g1: Pick a path?"));
        assert!(r.tasks.blocked.is_empty());

        let mut blocked = detached_state(autonomy(true, true, true));
        add_task(&mut blocked, "r1", "t1", &["codex:f12c2"], &[]);
        blocked.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Blocked;
        let r = idle_reason(&blocked.runs["r1"], NOW).expect("blocked conflict idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.blocked.len(), 1);
        assert_eq!(r.tasks.blocked[0].id, "t1");

        let mut review = detached_state(autonomy(true, true, true));
        add_task(&mut review, "r1", "t1", &["codex:f12c2"], &[]);
        review.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Review;
        let r = idle_reason(&review.runs["r1"], NOW).expect("review idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.review.len(), 1);
        assert_eq!(r.tasks.review[0].id, "t1");

        let mut intervention = detached_state(autonomy(true, true, true));
        add_task(&mut intervention, "r1", "t1", &["codex:f12c2"], &[]);
        f12_commit(&mut intervention, Cmd::TaskWait {
            run_id: "r1".into(), task_id: "t1".into(),
            wait: crate::orch::reducer::WaitRequest {
                kind: WaitKind::Intervention, resume_at: None, reason: "needs auth".into(),
                candidate: None, same_session: false,
            },
            now: 1_000_300,
        });
        let r = idle_reason(&intervention.runs["r1"], NOW).expect("intervention idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.tasks.waiting_intervention.len(), 1);
        assert_eq!(r.tasks.waiting_intervention[0].detail.as_deref(), Some("needs auth"));

        let mut quota_on = detached_state(autonomy(true, true, true));
        add_task(&mut quota_on, "r1", "t1", &["codex:f12c2"], &[]);
        wait(&mut quota_on, "r1", "t1", Some(NOW + 60_000), false);
        assert!(idle_reason(&quota_on.runs["r1"], NOW).is_none());

        for resume_at in [NOW + 60_000, NOW - 60_000] {
            let mut quota_off = detached_state(autonomy(false, true, true));
            add_task(&mut quota_off, "r1", "t1", &["codex:f12c2"], &[]);
            wait(&mut quota_off, "r1", "t1", Some(resume_at), false);
            let r = idle_reason(&quota_off.runs["r1"], NOW).expect("quota off idle");
            assert_eq!(r.reason, "idle");
            let group = if resume_at <= NOW { &r.tasks.resume_due } else { &r.tasks.waiting_no_resume };
            assert_eq!(group.len(), 1);
            assert_eq!(group[0].detail.as_deref(), Some("quota"));
        }

        let mut unintegrated = detached_state(autonomy(true, true, false));
        add_task(&mut unintegrated, "r1", "t1", &["codex:f12c2"], &[]);
        unintegrated.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Completed;
        let r = idle_reason(&unintegrated.runs["r1"], NOW).expect("unintegrated idle");
        assert_eq!(r.reason, "idle");
        assert_eq!(r.counts, IdleCounts { completed: 1, integrated: 0, cancelled: 0 });
        assert_eq!(r.tasks.unintegrated.len(), 1);
        assert_eq!(r.tasks.unintegrated[0].id, "t1");

        let mut finished = detached_state(autonomy(true, true, true));
        add_task(&mut finished, "r1", "t1", &["codex:f12c2"], &[]);
        add_task(&mut finished, "r1", "t2", &["codex:f12c2"], &[]);
        {
            let entry = finished.runs.get_mut("r1").unwrap();
            let t1 = entry.tasks.get_mut("t1").unwrap();
            t1.status = TaskStatus::Completed;
            t1.integrated_sha = Some("deadbeef".into());
            entry.tasks.get_mut("t2").unwrap().status = TaskStatus::Cancelled;
        }
        let r = idle_reason(&finished.runs["r1"], NOW).expect("finished report");
        assert_eq!(r.reason, "finished");
        assert_eq!(r.counts, IdleCounts { completed: 1, integrated: 1, cancelled: 1 });
        assert_eq!(r.tasks, IdleGroups::default());
    }

    fn add_exclusive_task(s: &mut State, run_id: &str, id: &str, exclusive: &[&str]) {
        f12_commit(s, Cmd::TaskCreate {
            run_id: run_id.into(), id: id.into(), title: id.into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec!["codex:f12c2".into()],
            verify: vec![], exclusive: exclusive.iter().map(|e| e.to_string()).collect(),
            role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(json!({"routing": 1, "pinned": false})));
    }

    #[test]
    fn plan_exclusive_lock_allows_only_one_start_per_name_per_tick() {
        let mut s = new_state("r1", autonomy(false, true, false));
        add_exclusive_task(&mut s, "r1", "t1", &["x"]);
        add_exclusive_task(&mut s, "r1", "t2", &["x"]);
        let actions = plan_at(&s, "r1", NOW);
        assert_eq!(actions.len(), 1, "only one of two tasks sharing exclusive 'x' planned: {actions:?}");
        assert!(matches!(actions[0], Action::Start { ref task_id, .. } if task_id == "t1"));
    }

    #[test]
    fn plan_exclusive_lock_blocks_failed_retry_when_name_held() {
        let mut s = new_state("r1", autonomy(true, true, false));
        add_exclusive_task(&mut s, "r1", "t1", &["y"]);
        add_exclusive_task(&mut s, "r1", "t2", &["y"]);
        let d = dispatch(&mut s, "r1", "t1", None);
        fail(&mut s, "r1", &d, FailureClass::QuotaExhausted);
        let d2 = dispatch(&mut s, "r1", "t2", None);
        s.runs.get_mut("r1").unwrap().dispatches.get_mut(&d2).unwrap().status = DispatchStatus::Running;
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t2").unwrap().status = TaskStatus::Dispatched;
        let actions = plan(TickInput {
            entry: &s.runs["r1"],
            now: NOW,
            is_live: &|_| true,
            locked_exclusive: &["y".to_string()],
        });
        assert!(actions.iter().all(|a| !matches!(a, Action::Retry { ref task_id, .. } if task_id == "t1")),
            "failed task with held exclusive must not be retried: {actions:?}");
    }

    fn add_in_place_task(s: &mut State, run_id: &str, id: &str) {
        f12_commit(s, Cmd::TaskCreate {
            run_id: run_id.into(), id: id.into(), title: id.into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec!["codex:f12c2".into()],
            verify: vec![], exclusive: vec![], role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(json!({"routing": 1, "pinned": false, "workspace": "main"})));
    }

    #[test]
    fn task_lock_names_includes_main_for_in_place_tasks() {
        let mut s = new_state("r1", Autonomy::default());
        add_in_place_task(&mut s, "r1", "t1");
        add_exclusive_task(&mut s, "r1", "t2", &["rail-build"]);
        f12_commit(&mut s, Cmd::TaskCreate {
            run_id: "r1".into(), id: "t3".into(), title: "t3".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec!["codex:f12c2".into()],
            verify: vec![], exclusive: vec![], role: None, review: false, owner: None, now: 1_000_000,
        }.with_fields(json!({"routing": 1})));
        let entry = &s.runs["r1"];
        let t1_names: Vec<&str> = task_lock_names(&entry.tasks["t1"]).collect();
        assert_eq!(t1_names, ["main"], "in-place task has implicit 'main' lock");
        let t2_names: Vec<&str> = task_lock_names(&entry.tasks["t2"]).collect();
        assert_eq!(t2_names, ["rail-build"], "worktree exclusive task");
        let t3_names: Vec<&str> = task_lock_names(&entry.tasks["t3"]).collect();
        assert!(t3_names.is_empty(), "plain worktree task has no locks");
    }

    #[test]
    fn deps_integrated_accepts_applied_in_place_dep() {
        let mut s = new_state("r1", Autonomy::default());
        add_in_place_task(&mut s, "r1", "t1");
        add_in_place_task(&mut s, "r1", "t2");
        {
            let entry = s.runs.get_mut("r1").unwrap();
            let t1 = entry.tasks.get_mut("t1").unwrap();
            t1.status = TaskStatus::Completed;
            t1.applied_in_place = true;
            let t2 = entry.tasks.get_mut("t2").unwrap();
            t2.deps = vec!["t1".into()];
        }
        let entry = &s.runs["r1"];
        assert!(deps_integrated(entry, &entry.tasks["t2"]), "applied_in_place dep counts as integrated");
    }

    #[test]
    fn plan_integrate_skips_applied_in_place_tasks() {
        let mut s = new_state("r1", autonomy(false, false, true));
        add_in_place_task(&mut s, "r1", "t1");
        let d = dispatch(&mut s, "r1", "t1", None);
        {
            let entry = s.runs.get_mut("r1").unwrap();
            let task = entry.tasks.get_mut("t1").unwrap();
            task.status = TaskStatus::Completed;
            task.applied_in_place = true;
            let disp = entry.dispatches.get_mut(&d).unwrap();
            disp.status = DispatchStatus::Reported;
            disp.outcome = Some("succeeded".into());
            disp.branch = Some("orch/r1/t1-1".into());
        }
        let actions = plan_at(&s, "r1", NOW);
        assert!(actions.is_empty(), "applied_in_place task must not be integrated: {actions:?}");
    }

    #[test]
    fn budget_cap_blocks_auto_retry_and_emits_once() {
        let mut s = new_state("r1", autonomy(true, true, false));
        add_task(&mut s, "r1", "t1", &["codex:f12c2"], &[]);
        {
            let entry = s.runs.get_mut("r1").unwrap();
            entry.tasks.get_mut("t1").unwrap().max_cost_usd = Some(1.0);
        }
        let d = dispatch(&mut s, "r1", "t1", None);
        {
            let entry = s.runs.get_mut("r1").unwrap();
            entry.dispatches.get_mut(&d).unwrap().cost_usd = Some(1.5);
        }
        fail(&mut s, "r1", &d, FailureClass::QualityFailed);
        let actions = plan_at(&s, "r1", NOW);
        let has_budget = actions.iter().any(|a| matches!(a, Action::BudgetExceeded { task_id } if task_id == "t1"));
        assert!(has_budget, "BudgetExceeded action expected when cost >= cap: {actions:?}");
        let no_retry = !actions.iter().any(|a| matches!(a, Action::Retry { task_id, .. } if task_id == "t1"));
        assert!(no_retry, "Retry must be suppressed when budget exceeded: {actions:?}");

        s.runs.get_mut("r1").unwrap().messages.push(Message {
            seq: 1, task_id: Some("t1".into()), dispatch_id: None,
            from: "coordinator".into(), to: "user".into(),
            msg_type: "budget_exceeded".into(), body: None, reply_to: None, at: NOW as u64, acked: false,
        });
        let actions2 = plan_at(&s, "r1", NOW);
        let no_dup = !actions2.iter().any(|a| matches!(a, Action::BudgetExceeded { task_id } if task_id == "t1"));
        assert!(no_dup, "BudgetExceeded must not be emitted twice: {actions2:?}");
    }
}
