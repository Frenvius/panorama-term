use std::collections::HashMap;
use serde_json::Value;
use crate::orch::model::*;

#[derive(Debug)]
pub enum OrchError {
    BadRequest(String),
    NotFound(String),
    Conflict(String),
    Limit(String),
}

impl OrchError {
    pub fn code(&self) -> &'static str {
        match self {
            OrchError::BadRequest(_) => "bad_request",
            OrchError::NotFound(_) => "not_found",
            OrchError::Conflict(_) => "conflict",
            OrchError::Limit(_) => "limit",
        }
    }
    pub fn message(&self) -> &str {
        match self {
            OrchError::BadRequest(m) | OrchError::NotFound(m)
            | OrchError::Conflict(m) | OrchError::Limit(m) => m,
        }
    }
}

pub enum ApplyResult {
    Ok(String, Value),
    Stale(String, Value),
    Err(OrchError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum IdleState {
    #[default]
    Active,
    Idle,
    Finished,
}

pub(crate) fn idle_fingerprint(msg_type: &str, body: &str) -> String {
    let mut value: Value = serde_json::from_str(body).unwrap_or_default();
    if let Some(groups) = value["tasks"].as_object_mut() {
        for tasks in groups.values_mut().filter_map(Value::as_array_mut) {
            tasks.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        }
    }
    serde_json::to_string(&(msg_type == "run_finished", value)).unwrap_or_default()
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct AutoActionFailure {
    pub action: String,
    pub error: String,
    pub at: u64,
    status: TaskStatus,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WakeReason {
    CoordinatorResume,
    #[serde(untagged)]
    Messages(Vec<String>),
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorWake {
    pub reason: WakeReason,
    pub message_count: usize,
    pub detached: bool,
    pub at: u64,
}

pub struct RunEntry {
    pub run: Run,
    pub tasks: HashMap<String, Task>,
    pub dispatches: HashMap<String, Dispatch>,
    pub messages: Vec<Message>,
    pub gates: HashMap<String, Gate>,
    pub task_count: u32,
    pub(crate) idle_state: IdleState,
    pub(crate) idle_fingerprint: Option<String>,
    pub(crate) auto_action_failures: HashMap<String, AutoActionFailure>,
    pub gate_count: u32,
    pub msg_seq: u32,
    pub event_seq: u32,
    pub coordinator_cost_baseline: Option<f64>,
    pub coordinator_cost_final: Option<f64>,
    pub(super) coordinator_wakes: Vec<CoordinatorWake>,
    pub(super) worktree_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl RunEntry {
    pub(crate) fn new(run: Run) -> Self {
        RunEntry {
            run,
            tasks: HashMap::new(),
            dispatches: HashMap::new(),
            messages: Vec::new(),
            gates: HashMap::new(),
            task_count: 0,
            idle_state: IdleState::Active,
            idle_fingerprint: None,
            auto_action_failures: HashMap::new(),
            gate_count: 0,
            msg_seq: 0,
            event_seq: 0,
            coordinator_cost_baseline: None,
            coordinator_cost_final: None,
            coordinator_wakes: Vec::new(),
            worktree_lock: Default::default(),
        }
    }

    pub fn next_task_id(&self) -> String {
        format!("t{}", self.task_count + 1)
    }

    pub fn next_gate_id(&self) -> String {
        format!("g{}", self.gate_count + 1)
    }

    pub fn next_msg_seq(&self) -> u32 {
        self.msg_seq + 1
    }

    pub fn task_ready(&self, task: &Task) -> bool {
        if task.status != TaskStatus::Pending {
            return false;
        }
        for dep in &task.deps {
            match self.tasks.get(dep) {
                Some(d) if d.status == TaskStatus::Completed => {}
                _ => return false,
            }
        }
        self.gates.values().all(|g| g.task_id != task.id || g.resolution.is_some())
    }

    pub fn running_dispatch_count(&self) -> usize {
        self.dispatches.values().filter(|d| d.status == DispatchStatus::Running).count()
    }

    pub(crate) fn latest_dispatch_for_task(&self, task_id: &str) -> Option<&Dispatch> {
        self.dispatches.values()
            .filter(|d| d.task_id == task_id)
            .max_by_key(|d| d.attempt)
    }
}

#[derive(Default)]
pub struct State {
    pub runs: HashMap<String, RunEntry>,
}

pub enum Cmd {
    RunCreate {
        id: String,
        title: String,
        repo: String,
        base_ref: Option<String>,
        base_sha: Option<String>,
        setup: Option<String>,
        coordinator_tile: Option<String>,
        limits: Limits,
        now: u64,
    },
    RunUpdate {
        run_id: String,
        title: String,
        now: u64,
    },
    RunSetCoordinator {
        run_id: String,
        tile_id: Option<String>,
        now: u64,
    },
    RunSetStatus {
        run_id: String,
        status: RunStatus,
        now: u64,
    },
    TaskCreate {
        run_id: String,
        id: String,
        title: String,
        spec: String,
        acceptance: Option<String>,
        deps: Vec<String>,
        profile: Option<String>,
        escalation: Vec<String>,
        verify: Vec<String>,
        role: Option<crate::orch::model::Role>,
        review: bool,
        owner: Option<String>,
        now: u64,
    },
    RunCoordinatorCost {
        run_id: String,
        cost_usd: f64,
        now: u64,
    },
    RunRecordCoordinatorBaseline {
        run_id: String,
        cost_usd: Option<f64>,
        now: u64,
    },
    TaskUpdate {
        run_id: String,
        task_id: String,
        status: Option<TaskStatus>,
        result: Option<String>,
        spec: Option<String>,
        acceptance: Option<String>,
        verify: Option<Vec<String>>,
        profile: Option<String>,
        escalation: Option<Vec<String>>,
        review: Option<bool>,
        owner: Option<String>,
        by: String,
        reason: Option<String>,
        now: u64,
    },
    Dispatch {
        run_id: String,
        id: String,
        task_id: String,
        attempt: u32,
        retry_of: Option<String>,
        profile: Option<String>,
        model: Option<String>,
        tile_id: Option<String>,
        owner: Option<String>,
        merge_branch: Option<String>,
        now: u64,
    },
    Send {
        run_id: String,
        seq: u32,
        from: String,
        to: Vec<String>,
        msg_type: String,
        task_id: Option<String>,
        dispatch_id: Option<String>,
        reply_to: Option<u32>,
        body: Option<String>,
        outcome: Option<String>,
        files_modified: Vec<String>,
        summary: Option<String>,
        now: u64,
    },
    Ack {
        run_id: String,
        seqs: Vec<u32>,
    },
    GateCreate {
        run_id: String,
        id: String,
        task_id: String,
        question: String,
        options: Vec<String>,
        msg_seq: u32,
        now: u64,
    },
    GateResolve {
        run_id: String,
        gate_id: String,
        resolution: String,
        now: u64,
    },
    DispatchUpdate {
        run_id: String,
        dispatch_id: String,
        status: DispatchStatus,
        summary: Option<String>,
        now: u64,
    },
    DispatchBindSession {
        run_id: String,
        dispatch_id: String,
        session: NativeSession,
        now: u64,
    },
    DispatchSetWorktree {
        run_id: String,
        dispatch_id: String,
        worktree: String,
        branch: String,
        now: u64,
    },
    DispatchRecordHead {
        run_id: String,
        dispatch_id: String,
        head_sha: String,
        files_modified: Vec<String>,
        now: u64,
    },
    TaskIntegrate {
        run_id: String,
        task_id: String,
        dispatch_id: String,
        integrated_sha: String,
        msg_seq: u32,
        now: u64,
    },
    TaskConflict {
        run_id: String,
        task_id: String,
        files: Vec<String>,
        branch: String,
        integration_tip: String,
        msg_seq: u32,
        now: u64,
    },
    DispatchAppendVerification {
        run_id: String,
        dispatch_id: String,
        command: String,
        exit_code: Option<i64>,
        timed_out: bool,
        tail: Vec<String>,
        head_sha: Option<String>,
        now: u64,
    },
    DispatchRecordCost {
        run_id: String,
        dispatch_id: String,
        cost_usd: Option<f64>,
        cost_source: String,
        now: u64,
    },
    TaskVerified {
        run_id: String,
        task_id: String,
        dispatch_id: String,
        msg_seq: u32,
        now: u64,
    },
    TaskVerificationFailed {
        run_id: String,
        task_id: String,
        dispatch_id: String,
        command: String,
        exit_code: Option<i64>,
        timed_out: bool,
        tail: String,
        msg_seq: u32,
        now: u64,
    },
    TaskReVerify {
        run_id: String,
        task_id: String,
        now: u64,
    },
    TaskWait {
        run_id: String,
        task_id: String,
        wait: WaitRequest,
        now: u64,
    },
    TaskResume {
        run_id: String,
        task_id: String,
        resume_id: String,
        now: u64,
    },
    TaskAutoActionFailure {
        run_id: String,
        task_id: String,
        failure: Option<(&'static str, String)>,
        expected: Option<(TaskStatus, u64)>,
        now: u64,
    },
    #[cfg(test)]
    TaskRecordFailure {
        run_id: String,
        task_id: String,
        dispatch_id: String,
        class: FailureClass,
        chain_index: Option<u32>,
        now: u64,
    },
    DispatchRevive {
        run_id: String,
        dispatch_id: String,
        now: u64,
    },
    RunSetCoordinatorWait {
        run_id: String,
        reset_at: Option<i64>,
        reason: String,
        now: u64,
    },
    RunClearCoordinatorWait {
        run_id: String,
        resume_id: String,
        now: u64,
    },
    RunCoordinatorWake {
        run_id: String,
        wake: CoordinatorWake,
    },
    RunIdle {
        run_id: String,
        msg_seq: u32,
        msg_type: String,
        body: String,
        now: u64,
    },
    RunActive {
        run_id: String,
        now: u64,
    },
    Extra {
        cmd: Box<Cmd>,
        fields: serde_json::Map<String, Value>,
    },
}

pub struct WaitRequest {
    pub kind: WaitKind,
    pub resume_at: Option<i64>,
    pub reason: String,
    pub candidate: Option<crate::orch::candidates::CandidateKey>,
    pub same_session: bool,
}

impl Cmd {
    pub fn with_fields(self, fields: Value) -> Cmd {
        let Value::Object(fields) = fields else { return self };
        Cmd::Extra { cmd: Box::new(self), fields }
    }
}

impl State {
    pub fn apply(&self, cmd: &Cmd) -> ApplyResult {
        match cmd {
            Cmd::Extra { cmd, fields } => match self.apply(cmd) {
                ApplyResult::Ok(kind, data) => ApplyResult::Ok(kind, merge_fields(data, fields)),
                ApplyResult::Stale(kind, data) => ApplyResult::Stale(kind, merge_fields(data, fields)),
                err => err,
            },

            Cmd::TaskWait { run_id, task_id, wait, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if !matches!(task.status, TaskStatus::Pending | TaskStatus::Failed) {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be pending or failed to wait".into()
                    ));
                }
                ApplyResult::Ok("task-wait".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "kind": wait.kind,
                    "resumeAt": wait.resume_at, "reason": wait.reason,
                    "candidate": wait.candidate, "sameSession": wait.same_session,
                    "resumeId": resume_id(task_id, task.attempts, wait.resume_at),
                    "now": now
                }))
            }

            Cmd::TaskResume { run_id, task_id, resume_id, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                let Some(wait) = task.wait.as_ref().filter(|_| task.status == TaskStatus::Waiting) else {
                    return ApplyResult::Err(OrchError::Conflict("task is not waiting".into()));
                };
                if wait.resume_id != *resume_id {
                    return ApplyResult::Err(OrchError::Conflict("resumeId mismatch".into()));
                }
                ApplyResult::Ok("task-resume".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "resumeId": resume_id, "now": now
                }))
            }

            Cmd::TaskAutoActionFailure { run_id, task_id, failure, expected, now } => {
                let Some(entry) = self.runs.get(run_id) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                let recorded = entry.auto_action_failures.contains_key(task_id);
                if failure.is_some() == recorded
                    || (failure.is_some() && !entry.run.detached)
                    || expected.as_ref().is_some_and(|(status, at)| task.status != *status || task.updated_at != *at)
                {
                    return ApplyResult::Err(OrchError::Conflict("autonomous action state changed".into()));
                }
                let failed_dispatch = failure.as_ref()
                    .filter(|(action, _)| *action == "continue" && task.status == TaskStatus::Dispatched)
                    .and_then(|_| entry.latest_dispatch_for_task(task_id))
                    .filter(|dispatch| dispatch.status == DispatchStatus::Running);
                let failure = failure.as_ref().map(|(action, error)| AutoActionFailure {
                    action: (*action).into(), error: error.clone(), at: *now,
                    status: if failed_dispatch.is_some() { TaskStatus::Failed } else { task.status.clone() },
                });
                ApplyResult::Ok("task-auto-action-failure".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "failure": failure,
                    "failedDispatchId": failed_dispatch.map(|dispatch| &dispatch.id), "now": now,
                }))
            }

            #[cfg(test)]
            Cmd::TaskRecordFailure { run_id, task_id, dispatch_id, class, chain_index, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if !entry.tasks.contains_key(task_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                }
                let Some(dispatch) = entry.dispatches.get(dispatch_id.as_str()).filter(|d| d.task_id == *task_id) else {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found for this task".into()));
                };
                if dispatch.failure_class.is_some() {
                    return ApplyResult::Err(OrchError::Conflict("failure already recorded".into()));
                }
                ApplyResult::Ok("task-record-failure".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "dispatchId": dispatch_id,
                    "class": class, "chainIndex": chain_index, "now": now
                }))
            }

            Cmd::RunIdle { run_id, msg_seq, msg_type, body, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let next = if msg_type == "run_finished" { IdleState::Finished } else { IdleState::Idle };
                let fingerprint = idle_fingerprint(msg_type, body);
                if entry.idle_state == next && entry.idle_fingerprint.as_deref() == Some(&fingerprint) {
                    return ApplyResult::Err(OrchError::Conflict("idle state unchanged".into()));
                }
                ApplyResult::Ok("run-idle".into(), serde_json::json!({
                    "runId": run_id, "msgSeq": msg_seq, "msgType": msg_type, "body": body, "fingerprint": fingerprint, "now": now
                }))
            }

            Cmd::RunActive { run_id, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.idle_state == IdleState::Active {
                    return ApplyResult::Err(OrchError::Conflict("run already active".into()));
                }
                ApplyResult::Ok("run-active".into(), serde_json::json!({
                    "runId": run_id, "now": now
                }))
            }

            Cmd::DispatchRevive { run_id, dispatch_id, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status != RunStatus::Active {
                    return ApplyResult::Err(OrchError::Conflict("run not active".into()));
                }
                let Some(d) = entry.dispatches.get(dispatch_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                };
                if d.status != DispatchStatus::Failed || d.outcome.is_some() {
                    return ApplyResult::Err(OrchError::Conflict("dispatch is not a failed live attempt".into()));
                }
                let is_latest = entry.latest_dispatch_for_task(&d.task_id).is_some_and(|l| l.id == d.id);
                let task_pending = entry.tasks.get(d.task_id.as_str()).is_some_and(|t| t.status == TaskStatus::Pending);
                if !is_latest || !task_pending {
                    return ApplyResult::Err(OrchError::Conflict("dispatch cannot be revived".into()));
                }
                if entry.running_dispatch_count() >= entry.run.limits.max_workers as usize {
                    return ApplyResult::Err(OrchError::Limit("max workers running".into()));
                }
                ApplyResult::Ok("dispatch-revive".into(), serde_json::json!({
                    "runId": run_id, "dispatchId": dispatch_id, "now": now
                }))
            }

            Cmd::RunSetCoordinatorWait { run_id, reset_at, reason, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                if entry.run.coordinator_wait.is_some() {
                    return ApplyResult::Err(OrchError::Conflict("coordinator already waiting".into()));
                }
                ApplyResult::Ok("run-coordinator-wait".into(), serde_json::json!({
                    "runId": run_id, "resetAt": reset_at, "reason": reason,
                    "resumeId": coordinator_resume_id(run_id, *reset_at), "now": now
                }))
            }

            Cmd::RunClearCoordinatorWait { run_id, resume_id, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let matches_wait = entry.run.coordinator_wait.as_ref().is_some_and(|w| w.resume_id == *resume_id);
                if !matches_wait {
                    return ApplyResult::Err(OrchError::Conflict("coordinator wait not set or resumeId mismatch".into()));
                }
                ApplyResult::Ok("run-coordinator-resume".into(), serde_json::json!({
                    "runId": run_id, "resumeId": resume_id, "now": now
                }))
            }

            Cmd::RunCoordinatorWake { run_id, wake } => {
                if !self.runs.contains_key(run_id) {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                }
                ApplyResult::Ok("run-coordinator-wake".into(), serde_json::json!({
                    "runId": run_id, "wake": wake, "now": wake.at,
                }))
            }

            Cmd::RunUpdate { run_id, title, now } => {
                if !self.runs.contains_key(run_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                }
                ApplyResult::Ok("run-update".into(), serde_json::json!({
                    "runId": run_id, "title": title, "now": now
                }))
            }

            Cmd::RunSetCoordinator { run_id, tile_id, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                ApplyResult::Ok("run-set-coordinator".into(), serde_json::json!({
                    "runId": run_id, "tileId": tile_id, "now": now
                }))
            }

            Cmd::RunCreate { id, title, repo, base_ref, base_sha, setup, coordinator_tile, limits, now } => {
                if self.runs.contains_key(id.as_str()) {
                    return ApplyResult::Err(OrchError::Conflict("run id already exists".into()));
                }
                ApplyResult::Ok("run-create".into(), serde_json::json!({
                    "id": id, "title": title, "repo": repo,
                    "baseRef": base_ref, "baseSha": base_sha, "setup": setup,
                    "coordinatorTile": coordinator_tile,
                    "limits": limits, "now": now
                }))
            }

            Cmd::RunSetStatus { run_id, status, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let cur = &entry.run.status;
                let ok = match (cur, status) {
                    (RunStatus::Active, RunStatus::Paused) => true,
                    (RunStatus::Paused, RunStatus::Active) => true,
                    (_, RunStatus::Cancelled) if !cur.is_terminal() => true,
                    (_, RunStatus::Completed) => {
                        if cur.is_terminal() {
                            return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                        }
                        let all_done = entry.tasks.values().all(|t| t.status.is_terminal());
                        if !all_done {
                            return ApplyResult::Err(OrchError::Conflict(
                                "not all tasks completed or cancelled".into()
                            ));
                        }
                        true
                    }
                    _ => false,
                };
                if !ok {
                    return ApplyResult::Err(OrchError::Conflict("invalid run status transition".into()));
                }
                ApplyResult::Ok("run-set-status".into(), serde_json::json!({
                    "runId": run_id, "status": status, "now": now
                }))
            }

            Cmd::TaskCreate { run_id, id, title, spec, acceptance, deps, profile, escalation, verify, role, review, owner, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                if entry.tasks.contains_key(id.as_str()) {
                    return ApplyResult::Err(OrchError::Conflict("task id already exists".into()));
                }
                for dep in deps {
                    if !entry.tasks.contains_key(dep.as_str()) {
                        return ApplyResult::Err(OrchError::BadRequest(
                            format!("unknown dep: {dep}")
                        ));
                    }
                }
                ApplyResult::Ok("task-create".into(), serde_json::json!({
                    "runId": run_id, "id": id, "title": title, "spec": spec,
                    "acceptance": acceptance, "deps": deps, "profile": profile,
                    "escalation": escalation, "verify": verify, "role": role,
                    "review": review, "owner": owner, "now": now
                }))
            }

            Cmd::TaskUpdate { run_id, task_id, status, result, spec, acceptance, verify, profile, escalation, review, owner, by, reason, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if let Some(new_status) = status {
                    let cur = &task.status;
                    let ok = matches!(
                        (cur, new_status),
                        (TaskStatus::Pending, TaskStatus::Cancelled)
                        | (TaskStatus::Pending, TaskStatus::Blocked)
                        | (TaskStatus::Blocked, TaskStatus::Pending)
                        | (TaskStatus::Blocked, TaskStatus::Cancelled)
                        | (TaskStatus::Failed, TaskStatus::Pending)
                        | (TaskStatus::Failed, TaskStatus::Cancelled)
                        | (TaskStatus::Verifying, TaskStatus::Completed)
                        | (TaskStatus::Verifying, TaskStatus::Failed)
                        | (TaskStatus::Dispatched, TaskStatus::Failed)
                        | (TaskStatus::Review, TaskStatus::Completed)
                        | (TaskStatus::Review, TaskStatus::Pending)
                        | (TaskStatus::Review, TaskStatus::Cancelled)
                        | (TaskStatus::Waiting, TaskStatus::Pending)
                        | (TaskStatus::Waiting, TaskStatus::Cancelled)
                    );
                    if !ok {
                        return ApplyResult::Err(OrchError::Conflict(format!(
                            "invalid task status transition {:?} -> {:?}", cur, new_status
                        )));
                    }
                    if matches!((cur, new_status), (TaskStatus::Blocked, TaskStatus::Pending)) {
                        let has_open_gate = entry.gates.values()
                            .any(|g| g.task_id == *task_id && g.resolution.is_none());
                        if has_open_gate {
                            return ApplyResult::Err(OrchError::Conflict(
                                "task has an open gate; resolve the gate to unblock".into()
                            ));
                        }
                    }
                }
                if let Some(owner) = owner {
                    if !matches!(owner.as_str(), "user" | "coordinator") || !matches!(by.as_str(), "user" | "coordinator") {
                        return ApplyResult::Err(OrchError::Conflict("invalid owner or actor".into()));
                    }
                    if owner == "coordinator" && by == "coordinator" && reason.as_deref().unwrap_or("").trim().is_empty() {
                        return ApplyResult::Err(OrchError::Conflict("coordinator ownership requires a reason".into()));
                    }
                }
                let field_edit = spec.is_some() || acceptance.is_some() || verify.is_some()
                    || profile.is_some() || escalation.is_some() || review.is_some();
                let end_status = status.as_ref().unwrap_or(&task.status);
                if field_edit && !matches!(end_status, TaskStatus::Pending | TaskStatus::Blocked | TaskStatus::Failed) {
                    return ApplyResult::Err(OrchError::Conflict(
                        "field edits only allowed when pending/blocked/failed".into()
                    ));
                }
                ApplyResult::Ok("task-update".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "status": status, "result": result,
                    "spec": spec, "acceptance": acceptance, "verify": verify,
                    "profile": profile, "escalation": escalation, "review": review, "owner": owner, "by": by, "reason": reason, "now": now
                }))
            }

            Cmd::Dispatch { run_id, id, task_id, attempt, retry_of, profile, model, tile_id, owner, merge_branch, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status != RunStatus::Active {
                    return ApplyResult::Err(OrchError::Conflict("run not active".into()));
                }
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                let resolving_conflict = merge_branch.is_some()
                    && task.status == TaskStatus::Blocked
                    && entry.gates.values().all(|g| g.task_id != task.id || g.resolution.is_some());
                let can_dispatch = entry.task_ready(task) || task.status == TaskStatus::Failed || resolving_conflict;
                if !can_dispatch {
                    return ApplyResult::Err(OrchError::Conflict("task not ready for dispatch".into()));
                }
                if task.routing == 1 {
                    if task.quality_attempts >= entry.run.limits.max_attempts {
                        return ApplyResult::Err(OrchError::Limit("max attempts reached".into()));
                    }
                    if task.operational_attempts >= entry.run.limits.max_operational_attempts {
                        return ApplyResult::Err(OrchError::Limit("max operational attempts reached".into()));
                    }
                } else if task.attempts >= entry.run.limits.max_attempts {
                    return ApplyResult::Err(OrchError::Limit("max attempts reached".into()));
                }
                if entry.running_dispatch_count() >= entry.run.limits.max_workers as usize {
                    return ApplyResult::Err(OrchError::Limit("max workers running".into()));
                }
                let expected_attempt = task.attempts + 1;
                if *attempt != expected_attempt {
                    return ApplyResult::Err(OrchError::Conflict(format!(
                        "attempt {attempt} != expected {expected_attempt}"
                    )));
                }
                if entry.dispatches.contains_key(id.as_str()) {
                    return ApplyResult::Err(OrchError::Conflict("dispatch id already exists".into()));
                }
                if let Some(d) = entry.latest_dispatch_for_task(task_id) {
                    if d.status == DispatchStatus::Running {
                        return ApplyResult::Err(OrchError::Conflict(
                            "task already has a running dispatch".into()
                        ));
                    }
                }
                ApplyResult::Ok("dispatch".into(), serde_json::json!({
                    "runId": run_id, "id": id, "taskId": task_id, "attempt": attempt,
                    "retryOf": retry_of, "profile": profile, "model": model,
                    "tileId": tile_id, "owner": owner, "mergeBranch": merge_branch, "now": now
                }))
            }

            Cmd::Send { run_id, seq, from, to, msg_type, task_id, dispatch_id, reply_to, body, outcome, files_modified, summary, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let is_dispatch_sender = !matches!(from.as_str(), "coordinator" | "user" | "brain");
                if is_dispatch_sender {
                    let Some(dispatch) = entry.dispatches.get(from.as_str()) else {
                        return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                    };
                    let latest = entry.latest_dispatch_for_task(&dispatch.task_id);
                    let is_latest = latest.map(|d| d.id == *from).unwrap_or(false);
                    if !is_latest {
                        return ApplyResult::Stale("send".into(), serde_json::json!({
                            "runId": run_id, "seq": seq, "from": from, "to": to,
                            "type": msg_type, "taskId": task_id, "dispatchId": dispatch_id,
                            "replyTo": reply_to, "body": body, "outcome": outcome,
                            "filesModified": files_modified, "summary": summary,
                            "now": now, "stale": true
                        }));
                    }
                    if msg_type == "worker_done" {
                        if dispatch.outcome.is_some() {
                            return ApplyResult::Err(OrchError::Conflict(
                                "worker_done already sent for this dispatch".into()
                            ));
                        }
                        if !matches!(outcome.as_deref(), Some("succeeded") | Some("failed")) {
                            return ApplyResult::Err(OrchError::BadRequest(
                                "outcome must be succeeded or failed".into()
                            ));
                        }
                        if dispatch.status == DispatchStatus::Failed {
                            let task_failed = entry.tasks.get(dispatch.task_id.as_str())
                                .map(|t| t.status == TaskStatus::Failed)
                                .unwrap_or(false);
                            if !task_failed {
                                return ApplyResult::Stale("send".into(), serde_json::json!({
                                    "runId": run_id, "seq": seq, "from": from, "to": to,
                                    "type": msg_type, "taskId": task_id, "dispatchId": dispatch_id,
                                    "replyTo": reply_to, "body": body, "outcome": outcome,
                                    "filesModified": files_modified, "summary": summary,
                                    "now": now, "stale": true
                                }));
                            }
                        }
                    }
                    if !dispatch.accepts_reports() {
                        return ApplyResult::Stale("send".into(), serde_json::json!({
                            "runId": run_id, "seq": seq, "from": from, "to": to,
                            "type": msg_type, "taskId": task_id, "dispatchId": dispatch_id,
                            "replyTo": reply_to, "body": body, "outcome": outcome,
                            "filesModified": files_modified, "summary": summary,
                            "now": now, "stale": true
                        }));
                    }
                }
                ApplyResult::Ok("send".into(), serde_json::json!({
                    "runId": run_id, "seq": seq, "from": from, "to": to,
                    "type": msg_type, "taskId": task_id, "dispatchId": dispatch_id,
                    "replyTo": reply_to, "body": body, "outcome": outcome,
                    "filesModified": files_modified, "summary": summary, "now": now
                }))
            }

            Cmd::Ack { run_id, seqs } => {
                if !self.runs.contains_key(run_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                }
                ApplyResult::Ok("ack".into(), serde_json::json!({
                    "runId": run_id, "seqs": seqs
                }))
            }

            Cmd::GateCreate { run_id, id, task_id, question, options, msg_seq, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if !matches!(task.status, TaskStatus::Pending | TaskStatus::Blocked | TaskStatus::Failed) {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be pending/blocked/failed for gate".into()
                    ));
                }
                if entry.gates.contains_key(id.as_str()) {
                    return ApplyResult::Err(OrchError::Conflict("gate id already exists".into()));
                }
                ApplyResult::Ok("gate-create".into(), serde_json::json!({
                    "runId": run_id, "id": id, "taskId": task_id, "question": question,
                    "options": options, "msgSeq": msg_seq, "now": now
                }))
            }

            Cmd::GateResolve { run_id, gate_id, resolution, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status.is_terminal() {
                    return ApplyResult::Err(OrchError::Conflict("run is terminal".into()));
                }
                let Some(gate) = entry.gates.get(gate_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("gate not found".into()));
                };
                if gate.resolution.is_some() {
                    return ApplyResult::Err(OrchError::Conflict("gate already resolved".into()));
                }
                ApplyResult::Ok("gate-resolve".into(), serde_json::json!({
                    "runId": run_id, "gateId": gate_id, "resolution": resolution, "now": now
                }))
            }
            Cmd::DispatchUpdate { run_id, dispatch_id, status, summary, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(d) = entry.dispatches.get(dispatch_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                };
                if d.status == DispatchStatus::Running || d.status == DispatchStatus::Reported {
                    let status_str = match status {
                        DispatchStatus::Stopped => "stopped",
                        DispatchStatus::Failed => "failed",
                        DispatchStatus::Stale => "stale",
                        _ => return ApplyResult::Err(OrchError::BadRequest("invalid terminal status".into())),
                    };
                    ApplyResult::Ok("dispatch-update".into(), serde_json::json!({
                        "runId": run_id, "dispatchId": dispatch_id,
                        "status": status_str, "summary": summary, "now": now
                    }))
                } else {
                    ApplyResult::Err(OrchError::Conflict("dispatch not in running or reported state".into()))
                }
            }

            Cmd::DispatchBindSession { run_id, dispatch_id, session, now } => {
                let dispatch = self.runs.get(run_id).and_then(|e| e.dispatches.get(dispatch_id));
                if !dispatch.is_some_and(|d| d.native_session.is_none()) {
                    return ApplyResult::Err(OrchError::Conflict("dispatch already bound or missing".into()));
                }
                ApplyResult::Ok("dispatch-bind-session".into(), serde_json::json!({
                    "runId": run_id, "dispatchId": dispatch_id, "session": session, "now": now
                }))
            }

            Cmd::DispatchSetWorktree { run_id, dispatch_id, worktree, branch, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if !entry.dispatches.contains_key(dispatch_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                }
                ApplyResult::Ok("dispatch-set-worktree".into(), serde_json::json!({
                    "runId": run_id, "dispatchId": dispatch_id,
                    "worktree": worktree, "branch": branch, "now": now
                }))
            }

            Cmd::DispatchRecordHead { run_id, dispatch_id, head_sha, files_modified, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if !entry.dispatches.contains_key(dispatch_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                }
                ApplyResult::Ok("dispatch-record-head".into(), serde_json::json!({
                    "runId": run_id, "dispatchId": dispatch_id,
                    "headSha": head_sha, "filesModified": files_modified, "now": now
                }))
            }

            Cmd::TaskIntegrate { run_id, task_id, dispatch_id, integrated_sha, msg_seq, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if task.status != TaskStatus::Completed {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be completed to integrate".into()
                    ));
                }
                let dispatch = entry.dispatches.get(dispatch_id.as_str());
                if dispatch.map(|d| d.task_id != *task_id).unwrap_or(true) {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found for this task".into()));
                }
                if dispatch.and_then(|d| d.branch.as_ref()).is_none() {
                    return ApplyResult::Err(OrchError::BadRequest("dispatch has no branch".into()));
                }
                ApplyResult::Ok("task-integrate".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "dispatchId": dispatch_id,
                    "integratedSha": integrated_sha, "msgSeq": msg_seq, "now": now
                }))
            }

            Cmd::TaskConflict { run_id, task_id, files, branch, integration_tip, msg_seq, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if task.status != TaskStatus::Completed {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be completed to record integration conflict".into()
                    ));
                }
                let hint = format!(
                    "Merge of {} into the run's integration branch conflicted in {} file(s). Nothing was changed on your branches. Run orch conflict-resolve to have a worker resolve it on top of the integration branch.",
                    task.title, files.len()
                );
                ApplyResult::Ok("task-conflict".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id,
                    "files": files, "branch": branch, "integrationTip": integration_tip,
                    "hint": hint, "msgSeq": msg_seq, "now": now
                }))
            }

            Cmd::DispatchAppendVerification { run_id, dispatch_id, command, exit_code, timed_out, tail, head_sha, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if !entry.dispatches.contains_key(dispatch_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                }
                ApplyResult::Ok("dispatch-append-verification".into(), serde_json::json!({
                    "runId": run_id, "dispatchId": dispatch_id,
                    "command": command, "exitCode": exit_code,
                    "timedOut": timed_out, "tail": tail, "headSha": head_sha, "now": now
                }))
            }

            Cmd::DispatchRecordCost { run_id, dispatch_id, cost_usd, cost_source, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if !entry.dispatches.contains_key(dispatch_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("dispatch not found".into()));
                }
                ApplyResult::Ok("dispatch-record-cost".into(), serde_json::json!({
                    "runId": run_id, "dispatchId": dispatch_id,
                    "costUsd": cost_usd, "costSource": cost_source, "now": now
                }))
            }

            Cmd::TaskVerified { run_id, task_id, dispatch_id, msg_seq, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if task.status != TaskStatus::Verifying {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be verifying".into()
                    ));
                }
                ApplyResult::Ok("task-verified".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "dispatchId": dispatch_id,
                    "review": task.review, "msgSeq": msg_seq, "now": now
                }))
            }

            Cmd::TaskVerificationFailed { run_id, task_id, dispatch_id, command, exit_code, timed_out, tail, msg_seq, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if task.status != TaskStatus::Verifying {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be verifying".into()
                    ));
                }
                let retry_same_entry = entry.run.corrective_retry
                    && entry.dispatches.get(dispatch_id).is_some_and(|d| !d.corrective_retry && d.merge_branch.is_none());
                ApplyResult::Ok("task-verification-failed".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "dispatchId": dispatch_id,
                    "command": command, "exitCode": exit_code, "timedOut": timed_out,
                    "tail": tail, "msgSeq": msg_seq, "now": now, "retrySameEntry": retry_same_entry
                }))
            }

            Cmd::TaskReVerify { run_id, task_id, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                if task.status != TaskStatus::Failed {
                    return ApplyResult::Err(OrchError::Conflict(
                        "task must be failed to re-verify".into()
                    ));
                }
                ApplyResult::Ok("task-re-verify".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "now": now
                }))
            }

            Cmd::RunCoordinatorCost { run_id, cost_usd, now } => {
                if !self.runs.contains_key(run_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                }
                ApplyResult::Ok("run-coordinator-cost".into(), serde_json::json!({
                    "runId": run_id, "costUsd": cost_usd, "now": now
                }))
            }

            Cmd::RunRecordCoordinatorBaseline { run_id, cost_usd, now } => {
                if !self.runs.contains_key(run_id.as_str()) {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                }
                ApplyResult::Ok("run-coordinator-baseline".into(), serde_json::json!({
                    "runId": run_id, "costUsd": cost_usd, "now": now
                }))
            }
        }
    }

    pub fn replay(&mut self, kind: &str, data: &Value) -> Result<(), String> {
        match kind {
            "task-auto-action-failure" => self.replay_auto_action_failure(data),
            "run-create" => self.replay_run_create(data),
            "run-update" => self.replay_run_update(data),
            "run-set-coordinator" => self.replay_run_set_coordinator(data),
            "run-set-status" => self.replay_run_set_status(data),
            "task-create" => self.replay_task_create(data),
            "task-update" => self.replay_task_update(data),
            "dispatch" => self.replay_dispatch(data),
            "send" => self.replay_send(data),
            "ack" => self.replay_ack(data),
            "gate-create" => self.replay_gate_create(data),
            "gate-resolve" => self.replay_gate_resolve(data),
            "dispatch-update" => self.replay_dispatch_update(data),
            "dispatch-set-worktree" => self.replay_dispatch_set_worktree(data),
            "dispatch-bind-session" => {
                if let Some(dispatch) = self.runs.get_mut(&str_field(data, "runId"))
                    .and_then(|e| e.dispatches.get_mut(&str_field(data, "dispatchId")))
                {
                    dispatch.native_session = parse_opt(data, "session");
                }
                Ok(())
            }
            "dispatch-record-head" => self.replay_dispatch_record_head(data),
            "task-integrate" => self.replay_task_integrate(data),
            "task-conflict" => self.replay_task_conflict(data),
            "dispatch-append-verification" => self.replay_dispatch_append_verification(data),
            "dispatch-record-cost" => self.replay_dispatch_record_cost(data),
            "task-verified" => self.replay_task_verified(data),
            "task-verification-failed" => self.replay_task_verification_failed(data),
            "task-re-verify" => self.replay_task_re_verify(data),
            "run-coordinator-cost" => self.replay_run_coordinator_cost(data),
            "run-coordinator-baseline" => self.replay_run_coordinator_baseline(data),
            "task-wait" => self.replay_task_wait(data),
            "task-resume" => self.replay_task_resume(data),
            "task-record-failure" => self.replay_task_record_failure(data),
            "dispatch-revive" => self.replay_dispatch_revive(data),
            "run-coordinator-wait" => self.replay_run_coordinator_wait(data),
            "run-coordinator-resume" => self.replay_run_coordinator_resume(data),
            "run-coordinator-wake" => {
                if let Some(entry) = self.runs.get_mut(&str_field(data, "runId")) {
                    let wake = serde_json::from_value(data["wake"].clone()).map_err(|e| e.to_string())?;
                    entry.coordinator_wakes.push(wake);
                }
                Ok(())
            }
            "run-idle" => self.replay_run_idle(data),
            "run-active" => self.replay_run_active(data),
            other => Err(format!("unknown event kind: {other}")),
        }?;
        if data["stale"] == true {
            return Ok(());
        }
        if let Some(entry) = self.runs.get_mut(data["runId"].as_str().unwrap_or_default()) {
            entry.auto_action_failures.retain(|id, failure| {
                entry.tasks.get(id).is_some_and(|task| task.status == failure.status)
            });
            if let Some(class) = parse_opt(data, "routedFailureClass") {
                let dispatch_id = str_field(data, if kind == "send" { "from" } else { "dispatchId" });
                let chain_index = entry.dispatches.get(&dispatch_id).and_then(|d| d.chain_index);
                Self::replay_failure_class(entry, &dispatch_id, (class, chain_index), u64_field(data, "now"));
            }
        }
        Ok(())
    }

    fn replay_auto_action_failure(&mut self, data: &Value) -> Result<(), String> {
        if let Some(dispatch_id) = data["failedDispatchId"].as_str() {
            self.replay_dispatch_update(&serde_json::json!({
                "runId": data["runId"], "dispatchId": dispatch_id, "status": "failed",
                "summary": data["failure"]["error"], "now": data["now"],
            }))?;
        }
        if let Some(entry) = self.runs.get_mut(&str_field(data, "runId")) {
            let task_id = str_field(data, "taskId");
            if data["failure"].is_null() {
                entry.auto_action_failures.remove(&task_id);
            } else {
                let failure = serde_json::from_value(data["failure"].clone()).map_err(|e| e.to_string())?;
                entry.auto_action_failures.insert(task_id, failure);
            }
        }
        Ok(())
    }

    fn replay_run_create(&mut self, data: &Value) -> Result<(), String> {
        let id = str_field(data, "id");
        if id.is_empty() { return Err("run-create: empty id".into()); }
        let limits: Limits = serde_json::from_value(data["limits"].clone()).unwrap_or_default();
        let now = u64_field(data, "now");
        let run = Run {
            id: id.clone(),
            title: str_field(data, "title"),
            repo: str_field(data, "repo"),
            base_ref: opt_str(data, "baseRef"),
            base_sha: opt_str(data, "baseSha"),
            setup: opt_str(data, "setup"),
            coordinator_tile: opt_str(data, "coordinatorTile"),
            status: RunStatus::Active,
            limits,
            autonomy: parse_autonomy(data.get("autonomy")).unwrap_or_default(),
            detached: data["detached"].as_bool().unwrap_or(false),
            corrective_retry: data["correctiveRetry"].as_bool().unwrap_or(true),
            coordinator_wait: None,
            created_at: now,
            updated_at: now,
        };
        self.runs.insert(id, RunEntry::new(run));
        Ok(())
    }

    fn replay_run_set_status(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-set-status: empty runId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let status: RunStatus = serde_json::from_value(data["status"].clone())
            .unwrap_or(RunStatus::Active);
        if status == RunStatus::Cancelled {
            for d in entry.dispatches.values_mut() {
                if d.status == DispatchStatus::Running {
                    d.status = DispatchStatus::Stopped;
                    d.ended_at = Some(now);
                }
            }
        }
        entry.run.status = status;
        entry.run.updated_at = now;
        Ok(())
    }

    fn replay_task_create(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let id = str_field(data, "id");
        if run_id.is_empty() { return Err("task-create: empty runId".into()); }
        if id.is_empty() { return Err("task-create: empty id".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let now = u64_field(data, "now");
        let role: Option<crate::orch::model::Role> = data.get("role")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let task = Task {
            id: id.clone(),
            title: str_field(data, "title"),
            spec: str_field(data, "spec"),
            acceptance: opt_str(data, "acceptance"),
            deps: str_arr(data, "deps"),
            profile: opt_str(data, "profile"),
            escalation: str_arr(data, "escalation"),
            verify: str_arr(data, "verify"),
            role,
            review: data["review"].as_bool().unwrap_or(false),
            owner: opt_str(data, "owner"),
            owner_reason: None,
            status: TaskStatus::Pending,
            result: None,
            integrated_sha: None,
            attempts: 0,
            routing: data["routing"].as_u64().unwrap_or(0) as u32,
            quality_floor: 0,
            quality_attempts: 0,
            operational_attempts: 0,
            pinned: data["pinned"].as_bool().unwrap_or(false),
            last_failure: None,
            wait: None,
            created_at: now,
            updated_at: now,
        };
        entry.task_count += 1;
        entry.tasks.insert(id, task);
        Ok(())
    }

    fn replay_task_update(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-update: empty runId".into()); }
        if task_id.is_empty() { return Err("task-update: empty taskId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };

        let new_status: Option<TaskStatus> = data.get("status")
            .filter(|v| !v.is_null())
            .and_then(|s| serde_json::from_value(s.clone()).ok());

        if new_status == Some(TaskStatus::Failed) {
            for d in entry.dispatches.values_mut() {
                if d.task_id == task_id && d.status == DispatchStatus::Running {
                    d.status = DispatchStatus::Stopped;
                    d.ended_at = Some(now);
                }
            }
        }

        entry.auto_action_failures.remove(&task_id);
        let Some(task) = entry.tasks.get_mut(&task_id) else { return Ok(()) };
        if let Some(s) = new_status {
            if s != TaskStatus::Waiting {
                task.wait = None;
            }
            task.status = s;
        }
        if let Some(v) = opt_val_str(data, "result") { task.result = Some(v); }
        if let Some(v) = opt_val_str(data, "spec") { task.spec = v; }
        if let Some(v) = opt_val_str(data, "acceptance") { task.acceptance = Some(v); }
        if let Some(arr) = opt_val_arr(data, "verify") { task.verify = arr; }
        if let Some(v) = opt_val_str(data, "profile") {
            task.profile = Some(v);
            task.pinned = true;
        }
        if let Some(arr) = opt_val_arr(data, "escalation") { task.escalation = arr; }
        if let Some(v) = data.get("review").filter(|v| !v.is_null()).and_then(|v| v.as_bool()) {
            task.review = v;
        }
        if let Some(owner) = opt_str(data, "owner") {
            task.owner = Some(owner);
            task.owner_reason = opt_str(data, "reason");
        }
        task.updated_at = now;
        Ok(())
    }

    fn replay_dispatch(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "id");
        if run_id.is_empty() { return Err("dispatch: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch: empty id".into()); }
        let task_id = str_field(data, "taskId");
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let attempt = data["attempt"].as_u64().unwrap_or(1) as u32;
        let dispatch = Dispatch {
            id: dispatch_id.clone(),
            task_id: task_id.clone(),
            attempt,
            retry_of: opt_str(data, "retryOf"),
            profile: opt_str(data, "profile"),
            model: opt_str(data, "model"),
            tile_id: opt_str(data, "tileId"),
            merge_branch: opt_str(data, "mergeBranch"),
            worktree: None,
            branch: None,
            status: DispatchStatus::Running,
            outcome: None,
            head_sha: None,
            files_modified: vec![],
            summary: None,
            report: None,
            started_at: now,
            ended_at: None,
            last_heartbeat: None,
            verifications: vec![],
            cost_usd: None,
            cost_source: None,
            account: opt_str(data, "account"),
            chain_index: data["chainIndex"].as_u64().map(|v| v as u32),
            trigger: parse_opt(data, "trigger"),
            decision: parse_opt(data, "decision"),
            failure_class: None,
            by: parse_opt(data, "by"),
            start_ref: None,
            native_home: opt_str(data, "nativeHome"),
            native_session: None,
            corrective_retry: data["correctiveRetry"].as_bool().unwrap_or(false),
            retry_same_entry: false,
            operational_wait: false,
        };
        if let Some(owner) = opt_str(data, "owner") {
            if let Some(task) = entry.tasks.get_mut(&task_id) {
                task.owner = Some(owner);
                task.owner_reason = None;
            }
        }
        entry.dispatches.insert(dispatch_id, dispatch);
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Dispatched;
            task.attempts = attempt;
            task.wait = None;
            task.updated_at = now;
        }
        Ok(())
    }

    fn replay_send(&mut self, data: &Value) -> Result<(), String> {
        if data["stale"].as_bool().unwrap_or(false) {
            return Ok(());
        }
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("send: empty runId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };

        let from = str_field(data, "from");
        let msg_type = data["type"].as_str().unwrap_or("").to_string();

        if msg_type == "heartbeat" {
            if let Some(dispatch) = entry.dispatches.get_mut(&from) {
                dispatch.last_heartbeat = Some(now);
            }
            return Ok(());
        }

        if msg_type == "worker_done" {
            let outcome = data["outcome"].as_str().unwrap_or("").to_string();
            if let Some(dispatch) = entry.dispatches.get_mut(&from) {
                dispatch.status = if outcome == "succeeded" {
                    DispatchStatus::Reported
                } else {
                    DispatchStatus::Failed
                };
                dispatch.outcome = Some(outcome.clone());
                dispatch.ended_at = Some(now);
                dispatch.files_modified = str_arr(data, "filesModified");
                dispatch.summary = opt_str(data, "summary");
                dispatch.report = opt_str(data, "body");
                if let Some(class) = parse_opt(data, "failureClass") {
                    dispatch.failure_class = Some(class);
                }
            }
            let task_id = entry.dispatches.get(&from).map(|d| d.task_id.clone());
            if let Some(task_id) = task_id {
                if let Some(task) = entry.tasks.get_mut(&task_id) {
                    task.status = if outcome == "succeeded" {
                        if !task.verify.is_empty() {
                            TaskStatus::Verifying
                        } else if task.review {
                            TaskStatus::Review
                        } else {
                            TaskStatus::Completed
                        }
                    } else {
                        TaskStatus::Failed
                    };
                    task.updated_at = now;
                }
            }
        }

        let seq = data["seq"].as_u64().unwrap_or(0) as u32;
        let recipients = to_recipients(&data["to"]);
        for (i, recipient) in recipients.iter().enumerate() {
            let msg = Message {
                seq: seq.saturating_add(i as u32),
                from: from.clone(),
                to: recipient.clone(),
                msg_type: msg_type.clone(),
                task_id: opt_str(data, "taskId"),
                dispatch_id: opt_str(data, "dispatchId"),
                reply_to: data["replyTo"].as_u64().map(|v| v as u32),
                body: opt_str(data, "body"),
                at: now,
                acked: false,
            };
            entry.messages.push(msg);
        }
        let last_seq = seq.saturating_add(recipients.len().saturating_sub(1) as u32);
        if last_seq > entry.msg_seq {
            entry.msg_seq = last_seq;
        }
        Ok(())
    }

    fn replay_ack(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("ack: empty runId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let seqs: Vec<u32> = data["seqs"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_u64().map(|n| n as u32)).collect())
            .unwrap_or_default();
        for msg in &mut entry.messages {
            if seqs.contains(&msg.seq) {
                msg.acked = true;
            }
        }
        Ok(())
    }

    fn replay_gate_create(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let gate_id = str_field(data, "id");
        if run_id.is_empty() { return Err("gate-create: empty runId".into()); }
        if gate_id.is_empty() { return Err("gate-create: empty id".into()); }
        let task_id = str_field(data, "taskId");
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let gate = Gate {
            id: gate_id.clone(),
            task_id: task_id.clone(),
            question: str_field(data, "question"),
            options: str_arr(data, "options"),
            resolution: None,
            created_at: now,
            resolved_at: None,
        };
        entry.gate_count += 1;
        entry.gates.insert(gate_id.clone(), gate);
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Blocked;
            task.updated_at = now;
        }
        let msg_seq = data["msgSeq"].as_u64().unwrap_or(0) as u32;
        let msg = Message {
            seq: msg_seq,
            from: "gate".to_string(),
            to: "coordinator".to_string(),
            msg_type: "gate".to_string(),
            task_id: Some(task_id),
            dispatch_id: None,
            reply_to: None,
            body: Some(gate_id),
            at: now,
            acked: false,
        };
        entry.messages.push(msg);
        if msg_seq > entry.msg_seq {
            entry.msg_seq = msg_seq;
        }
        Ok(())
    }

    fn replay_gate_resolve(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let gate_id = str_field(data, "gateId");
        if run_id.is_empty() { return Err("gate-resolve: empty runId".into()); }
        if gate_id.is_empty() { return Err("gate-resolve: empty gateId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let task_id = entry.gates.get(&gate_id).map(|g| g.task_id.clone());
        if let Some(gate) = entry.gates.get_mut(&gate_id) {
            gate.resolution = Some(str_field(data, "resolution"));
            gate.resolved_at = Some(now);
        }
        if let Some(task_id) = task_id {
            let open = entry.gates.values()
                .any(|g| g.task_id == task_id && g.resolution.is_none());
            if !open {
                if let Some(task) = entry.tasks.get_mut(&task_id) {
                    if task.status == TaskStatus::Blocked {
                        task.status = TaskStatus::Pending;
                        task.updated_at = now;
                    }
                }
            }
        }
        Ok(())
    }

    fn replay_dispatch_set_worktree(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "dispatchId");
        if run_id.is_empty() { return Err("dispatch-set-worktree: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch-set-worktree: empty dispatchId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(d) = entry.dispatches.get_mut(&dispatch_id) {
            let worktree = opt_str(data, "worktree");
            if d.worktree != worktree {
                d.native_session = None;
            }
            d.worktree = worktree;
            d.branch = opt_str(data, "branch");
            d.start_ref = opt_str(data, "startRef");
        }
        Ok(())
    }

    fn replay_dispatch_record_head(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "dispatchId");
        if run_id.is_empty() { return Err("dispatch-record-head: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch-record-head: empty dispatchId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(d) = entry.dispatches.get_mut(&dispatch_id) {
            let sha = str_field(data, "headSha");
            if !sha.is_empty() {
                d.head_sha = Some(sha);
            }
            let files = str_arr(data, "filesModified");
            if !files.is_empty() {
                d.files_modified = files;
            }
        }
        Ok(())
    }

    fn replay_task_integrate(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-integrate: empty runId".into()); }
        if task_id.is_empty() { return Err("task-integrate: empty taskId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.integrated_sha = opt_str(data, "integratedSha");
            task.updated_at = now;
        }
        let msg_seq = data["msgSeq"].as_u64().unwrap_or(0) as u32;
        let dispatch_id = opt_str(data, "dispatchId");
        let msg = Message {
            seq: msg_seq,
            from: "brain".to_string(),
            to: "coordinator".to_string(),
            msg_type: "task_integrated".to_string(),
            task_id: Some(task_id),
            dispatch_id,
            reply_to: None,
            body: Some(format!(
                "{}\nMerged into orch/{run_id}/integration (your main branch is untouched)",
                str_field(data, "integratedSha")
            )),
            at: now,
            acked: false,
        };
        entry.messages.push(msg);
        if msg_seq > entry.msg_seq {
            entry.msg_seq = msg_seq;
        }
        Ok(())
    }

    fn replay_task_conflict(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-conflict: empty runId".into()); }
        if task_id.is_empty() { return Err("task-conflict: empty taskId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let files = str_arr(data, "files");
        let result_str = files.join(", ");
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Blocked;
            task.result = Some(result_str);
            task.updated_at = now;
        }
        let msg_seq = data["msgSeq"].as_u64().unwrap_or(0) as u32;
        let msg = Message {
            seq: msg_seq,
            from: "brain".to_string(),
            to: "coordinator".to_string(),
            msg_type: "integration_conflict".to_string(),
            task_id: Some(task_id),
            dispatch_id: None,
            reply_to: None,
            body: Some(serde_json::json!({
                "files": data["files"],
                "branch": data["branch"],
                "integrationTip": data["integrationTip"],
                "hint": data["hint"],
            }).to_string()),
            at: now,
            acked: false,
        };
        entry.messages.push(msg);
        if msg_seq > entry.msg_seq {
            entry.msg_seq = msg_seq;
        }
        Ok(())
    }

    fn replay_dispatch_update(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "dispatchId");
        if run_id.is_empty() { return Err("dispatch-update: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch-update: empty dispatchId".into()); }
        let now = u64_field(data, "now");
        let status_str = str_field(data, "status");
        let status = match status_str.as_str() {
            "stopped" => DispatchStatus::Stopped,
            "failed" => DispatchStatus::Failed,
            "stale" => DispatchStatus::Stale,
            _ => return Err(format!("dispatch-update: unknown status {status_str}")),
        };
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(d) = entry.dispatches.get_mut(&dispatch_id) {
            let task_id = d.task_id.clone();
            d.status = status;
            d.ended_at = Some(now);
            if let Some(summary) = opt_str(data, "summary") {
                d.summary = Some(summary);
            }
            if let Some(class) = parse_opt(data, "failureClass") {
                d.failure_class = Some(class);
            }
            if let Some(task) = entry.tasks.get_mut(&task_id) {
                if !task.status.is_terminal() {
                    task.status = TaskStatus::Failed;
                    task.updated_at = now;
                }
            }
        }
        Ok(())
    }

    fn replay_dispatch_append_verification(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "dispatchId");
        if run_id.is_empty() { return Err("dispatch-append-verification: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch-append-verification: empty dispatchId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let now = u64_field(data, "now");
        let tail: Vec<String> = data["tail"].as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
            .unwrap_or_default();
        let result = crate::orch::model::VerificationResult {
            command: str_field(data, "command"),
            exit_code: data["exitCode"].as_i64(),
            timed_out: data["timedOut"].as_bool().unwrap_or(false),
            tail,
            head_sha: opt_str(data, "headSha"),
            at: now,
        };
        if let Some(d) = entry.dispatches.get_mut(&dispatch_id) {
            d.verifications.push(result);
        }
        Ok(())
    }

    fn replay_dispatch_record_cost(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "dispatchId");
        if run_id.is_empty() { return Err("dispatch-record-cost: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch-record-cost: empty dispatchId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(d) = entry.dispatches.get_mut(&dispatch_id) {
            d.cost_usd = data["costUsd"].as_f64();
            d.cost_source = opt_str(data, "costSource");
        }
        Ok(())
    }

    fn replay_task_verified(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-verified: empty runId".into()); }
        if task_id.is_empty() { return Err("task-verified: empty taskId".into()); }
        let now = u64_field(data, "now");
        let review = data["review"].as_bool().unwrap_or(false);
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = if review { TaskStatus::Review } else { TaskStatus::Completed };
            task.updated_at = now;
        }
        let msg_seq = data["msgSeq"].as_u64().unwrap_or(0) as u32;
        let dispatch_id = opt_str(data, "dispatchId");
        let msg = Message {
            seq: msg_seq,
            from: "brain".to_string(),
            to: "coordinator".to_string(),
            msg_type: "verification_passed".to_string(),
            task_id: Some(task_id),
            dispatch_id,
            reply_to: None,
            body: None,
            at: now,
            acked: false,
        };
        entry.messages.push(msg);
        if msg_seq > entry.msg_seq {
            entry.msg_seq = msg_seq;
        }
        Ok(())
    }

    fn replay_task_verification_failed(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-verification-failed: empty runId".into()); }
        if task_id.is_empty() { return Err("task-verification-failed: empty taskId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Failed;
            task.updated_at = now;
        }
        let msg_seq = data["msgSeq"].as_u64().unwrap_or(0) as u32;
        let dispatch_id = opt_str(data, "dispatchId");
        if let Some(d) = dispatch_id.as_ref().and_then(|id| entry.dispatches.get_mut(id)) {
            if let Some(class) = parse_opt(data, "failureClass") {
                d.failure_class = Some(class);
            }
            d.retry_same_entry = data["retrySameEntry"].as_bool().unwrap_or(false);
        }
        let body = serde_json::json!({
            "command": str_field(data, "command"),
            "exitCode": data["exitCode"],
            "timedOut": data["timedOut"].as_bool().unwrap_or(false),
            "tail": str_field(data, "tail")
        }).to_string();
        let msg = Message {
            seq: msg_seq,
            from: "brain".to_string(),
            to: "coordinator".to_string(),
            msg_type: "verification_failed".to_string(),
            task_id: Some(task_id),
            dispatch_id,
            reply_to: None,
            body: Some(body),
            at: now,
            acked: false,
        };
        entry.messages.push(msg);
        if msg_seq > entry.msg_seq {
            entry.msg_seq = msg_seq;
        }
        Ok(())
    }

    fn replay_task_re_verify(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-re-verify: empty runId".into()); }
        if task_id.is_empty() { return Err("task-re-verify: empty taskId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Verifying;
            task.updated_at = now;
        }
        Ok(())
    }

    fn replay_run_update(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-update: empty runId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(title) = opt_str(data, "title") {
            entry.run.title = title;
        }
        if let Some(autonomy) = parse_autonomy(data.get("autonomy")) {
            entry.run.autonomy = autonomy;
        }
        if let Some(detached) = data["detached"].as_bool() {
            entry.run.detached = detached;
        }
        if let Some(enabled) = data["correctiveRetry"].as_bool() {
            entry.run.corrective_retry = enabled;
        }
        if let Some(n) = data["maxOperationalAttempts"].as_u64() {
            entry.run.limits.max_operational_attempts = n as u32;
        }
        entry.run.updated_at = now;
        Ok(())
    }

    fn replay_task_wait(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-wait: empty runId".into()); }
        if task_id.is_empty() { return Err("task-wait: empty taskId".into()); }
        let Some(kind) = parse_opt::<WaitKind>(data, "kind") else {
            return Err("task-wait: invalid kind".into());
        };
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if matches!(kind, WaitKind::Quota | WaitKind::Availability) {
            if let Some(dispatch) = entry.dispatches.values_mut()
                .filter(|d| d.task_id == task_id).max_by_key(|d| d.attempt)
                .filter(|d| d.failure_class.is_some_and(|class| class.counter() == FailureCounter::Operational))
            {
                dispatch.operational_wait = true;
            }
        }
        let Some(task) = entry.tasks.get_mut(&task_id) else { return Ok(()) };
        let resume_at = data["resumeAt"].as_i64();
        task.status = TaskStatus::Waiting;
        task.wait = Some(TaskWait {
            kind,
            resume_at,
            reason: str_field(data, "reason"),
            candidate: parse_opt(data, "candidate"),
            same_session: data["sameSession"].as_bool().unwrap_or(false),
            resume_id: opt_str(data, "resumeId")
                .unwrap_or_else(|| resume_id(&task_id, task.attempts, resume_at)),
            since: now as i64,
        });
        task.updated_at = now;
        Ok(())
    }

    fn replay_task_resume(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-resume: empty runId".into()); }
        if task_id.is_empty() { return Err("task-resume: empty taskId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let Some(task) = entry.tasks.get_mut(&task_id) else { return Ok(()) };
        if task.status != TaskStatus::Waiting { return Ok(()) }
        task.status = TaskStatus::Pending;
        task.wait = None;
        task.updated_at = now;
        Ok(())
    }

    fn replay_task_record_failure(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let task_id = str_field(data, "taskId");
        if run_id.is_empty() { return Err("task-record-failure: empty runId".into()); }
        if task_id.is_empty() { return Err("task-record-failure: empty taskId".into()); }
        let Some(class) = parse_opt::<FailureClass>(data, "class") else {
            return Err("task-record-failure: invalid class".into());
        };
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let chain_index = data["chainIndex"].as_u64().map(|v| v as u32);
        Self::replay_failure_class(entry, &str_field(data, "dispatchId"), (class, chain_index), now);
        Ok(())
    }

    fn replay_failure_class(entry: &mut RunEntry, dispatch_id: &str, failure: (FailureClass, Option<u32>), now: u64) {
        let (class, chain_index) = failure;
        let Some(dispatch) = entry.dispatches.get_mut(dispatch_id) else { return };
        if dispatch.failure_class.is_some() {
            return;
        }
        dispatch.failure_class = Some(class);
        let Some(task) = entry.tasks.get_mut(&dispatch.task_id) else { return };
        match class.counter() {
            FailureCounter::Operational => {
                task.operational_attempts = task.operational_attempts.saturating_add(1);
            }
            FailureCounter::Quality => {
                task.quality_attempts = task.quality_attempts.saturating_add(1);
                task.quality_floor = crate::orch::routing::floor_after_quality_failure(task.quality_floor, chain_index);
            }
            FailureCounter::None => {}
        }
        task.last_failure = Some(class);
        task.updated_at = now;
    }

    fn replay_dispatch_revive(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let dispatch_id = str_field(data, "dispatchId");
        if run_id.is_empty() { return Err("dispatch-revive: empty runId".into()); }
        if dispatch_id.is_empty() { return Err("dispatch-revive: empty dispatchId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let Some(d) = entry.dispatches.get_mut(&dispatch_id) else { return Ok(()) };
        d.status = DispatchStatus::Running;
        d.ended_at = None;
        d.failure_class = None;
        let task_id = d.task_id.clone();
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Dispatched;
            task.updated_at = now;
        }
        Ok(())
    }

    fn replay_run_coordinator_wait(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-coordinator-wait: empty runId".into()); }
        let now = u64_field(data, "now");
        let reset_at = data["resetAt"].as_i64();
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        entry.run.coordinator_wait = Some(CoordinatorWait {
            reset_at,
            since: now as i64,
            reason: str_field(data, "reason"),
            resume_id: opt_str(data, "resumeId").unwrap_or_else(|| coordinator_resume_id(&run_id, reset_at)),
        });
        entry.run.updated_at = now;
        Ok(())
    }

    fn replay_run_coordinator_resume(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-coordinator-resume: empty runId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        entry.run.coordinator_wait = None;
        entry.run.updated_at = now;
        Ok(())
    }

    fn replay_run_set_coordinator(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-set-coordinator: empty runId".into()); }
        let now = u64_field(data, "now");
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        entry.run.coordinator_tile = opt_str(data, "tileId");
        entry.run.updated_at = now;
        Ok(())
    }

    fn replay_run_coordinator_cost(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-coordinator-cost: empty runId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        if let Some(cost) = data["costUsd"].as_f64() {
            entry.coordinator_cost_final = Some(cost);
        }
        Ok(())
    }

    fn replay_run_coordinator_baseline(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-coordinator-baseline: empty runId".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        entry.coordinator_cost_baseline = data["costUsd"].as_f64();
        Ok(())
    }

    fn replay_run_idle(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        let msg_type = str_field(data, "msgType");
        if run_id.is_empty() { return Err("run-idle: empty runId".into()); }
        if msg_type.is_empty() { return Err("run-idle: empty msgType".into()); }
        let Some(entry) = self.runs.get_mut(&run_id) else { return Ok(()) };
        let msg_seq = data["msgSeq"].as_u64().map(|v| v as u32).unwrap_or_else(|| entry.next_msg_seq());
        entry.idle_state = if msg_type == "run_finished" { IdleState::Finished } else { IdleState::Idle };
        entry.idle_fingerprint = Some(data["fingerprint"].as_str().map(str::to_owned).unwrap_or_else(|| idle_fingerprint(&msg_type, data["body"].as_str().unwrap_or(""))));
        entry.msg_seq = entry.msg_seq.max(msg_seq);
        entry.messages.push(Message {
            seq: msg_seq,
            from: "brain".into(),
            to: "coordinator".into(),
            msg_type,
            task_id: None,
            dispatch_id: None,
            reply_to: None,
            body: opt_str(data, "body"),
            at: u64_field(data, "now"),
            acked: false,
        });
        Ok(())
    }

    fn replay_run_active(&mut self, data: &Value) -> Result<(), String> {
        let run_id = str_field(data, "runId");
        if run_id.is_empty() { return Err("run-active: empty runId".into()); }
        if let Some(entry) = self.runs.get_mut(&run_id) {
            entry.idle_state = IdleState::Active;
        }
        Ok(())
    }
}

fn merge_fields(mut data: Value, fields: &serde_json::Map<String, Value>) -> Value {
    if let Some(obj) = data.as_object_mut() {
        for (k, v) in fields {
            obj.insert(k.clone(), v.clone());
        }
    }
    data
}

fn parse_opt<T: serde::de::DeserializeOwned>(data: &Value, key: &str) -> Option<T> {
    data.get(key)
        .filter(|v| !v.is_null())
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn parse_autonomy(v: Option<&Value>) -> Option<Autonomy> {
    v.filter(|v| !v.is_null()).and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn str_field(data: &Value, key: &str) -> String {
    data[key].as_str().unwrap_or("").to_string()
}

fn u64_field(data: &Value, key: &str) -> u64 {
    data[key].as_u64().unwrap_or(0)
}

fn opt_str(data: &Value, key: &str) -> Option<String> {
    data[key].as_str().map(str::to_string)
}

fn opt_val_str(data: &Value, key: &str) -> Option<String> {
    let v = data.get(key)?;
    if v.is_null() { return None; }
    v.as_str().map(str::to_string)
}

fn opt_val_arr(data: &Value, key: &str) -> Option<Vec<String>> {
    let v = data.get(key)?;
    if v.is_null() { return None; }
    serde_json::from_value(v.clone()).ok()
}

fn str_arr(data: &Value, key: &str) -> Vec<String> {
    data[key].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).map(str::to_string).collect())
        .unwrap_or_default()
}

fn to_recipients(val: &Value) -> Vec<String> {
    if let Some(arr) = val.as_array() {
        arr.iter().filter_map(|v| v.as_str()).map(str::to_string).collect()
    } else if let Some(s) = val.as_str() {
        vec![s.to_string()]
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> u64 { 1_000_000 }

    fn mk_run(state: &mut State, id: &str) {
        let cmd = Cmd::RunCreate {
            id: id.to_string(),
            title: "run".into(),
            repo: "/tmp/r".into(),
            base_ref: None,
            base_sha: None,
            setup: None,
            coordinator_tile: None,
            limits: Limits::default(),
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("mk_run replay"),
            _ => panic!("run-create failed"),
        }
    }

    fn mk_task(state: &mut State, run_id: &str, task_id: &str, deps: Vec<String>) {
        let cmd = Cmd::TaskCreate {
            run_id: run_id.into(),
            id: task_id.into(),
            title: "task".into(),
            spec: "spec".into(),
            acceptance: None,
            deps,
            profile: None,
            escalation: vec![],
            verify: vec![],
            role: None,
            review: false,
            owner: None,
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("mk_task replay"),
            _ => panic!("task-create failed"),
        }
    }

    fn mk_dispatch(state: &mut State, run_id: &str, task_id: &str, attempt: u32) -> String {
        let id = format!("{run_id}.{task_id}.{attempt}");
        let cmd = Cmd::Dispatch {
            run_id: run_id.into(),
            id: id.clone(),
            task_id: task_id.into(),
            attempt,
            retry_of: None,
            profile: None,
            model: None,
            tile_id: None,
            owner: None,
            merge_branch: None,
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("mk_dispatch replay"),
            _ => panic!("dispatch failed"),
        }
        id
    }

    fn worker_done(state: &mut State, run_id: &str, dispatch_id: &str, outcome: &str, seq: u32) {
        let cmd = Cmd::Send {
            run_id: run_id.into(),
            seq,
            from: dispatch_id.into(),
            to: vec!["coordinator".into()],
            msg_type: "worker_done".into(),
            task_id: None,
            dispatch_id: None,
            reply_to: None,
            body: None,
            outcome: Some(outcome.into()),
            files_modified: vec![],
            summary: None,
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) | ApplyResult::Stale(k, d) => {
                state.replay(&k, &d).expect("worker_done replay")
            }
            ApplyResult::Err(e) => panic!("worker_done failed: {}", e.message()),
        }
    }

    #[test]
    fn worker_report_survives_event_replay() {
        let mut state = State::default();
        mk_run(&mut state, "r1");
        mk_task(&mut state, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut state, "r1", "t1", 1);
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 1, from: did.clone(), to: vec!["coordinator".into()],
            msg_type: "worker_done".into(), task_id: None, dispatch_id: Some(did.clone()),
            reply_to: None, body: Some("# Full report\nEvidence".into()), outcome: Some("succeeded".into()),
            files_modified: vec![], summary: Some("Done".into()), now: ts(),
        };
        let (kind, event) = match state.apply(&cmd) { ApplyResult::Ok(k, d) => (k, d), _ => panic!("send failed") };
        let event: Value = serde_json::from_str(&serde_json::to_string(&event).unwrap()).unwrap();
        state.replay(&kind, &event).unwrap();
        let entry = &state.runs["r1"];
        assert_eq!(entry.dispatches[&did].report.as_deref(), Some("# Full report\nEvidence"));
        assert_eq!(entry.messages.last().unwrap().body.as_deref(), Some("# Full report\nEvidence"));
    }

    fn mk_gate(state: &mut State, run_id: &str, gate_id: &str, task_id: &str, msg_seq: u32) {
        let cmd = Cmd::GateCreate {
            run_id: run_id.into(),
            id: gate_id.into(),
            task_id: task_id.into(),
            question: "question?".into(),
            options: vec![],
            msg_seq,
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("mk_gate replay"),
            _ => panic!("gate-create failed"),
        }
    }

    #[test]
    fn task_deps_unknown_dep_rejected() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::TaskCreate {
            run_id: "r1".into(),
            id: "t1".into(),
            title: "t".into(),
            spec: "s".into(),
            acceptance: None,
            deps: vec!["t99".into()],
            profile: None,
            escalation: vec![],
            verify: vec![],
            role: None,
            review: false,
            owner: None,
            now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::BadRequest(_))));
    }

    #[test]
    fn task_deps_ready_only_after_all_deps_completed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_task(&mut s, "r1", "t2", vec!["t1".into()]);
        let not_ready = {
            let e = s.runs.get("r1").unwrap();
            !e.task_ready(e.tasks.get("t2").unwrap())
        };
        assert!(not_ready);

        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);

        let ready = {
            let e = s.runs.get("r1").unwrap();
            e.task_ready(e.tasks.get("t2").unwrap())
        };
        assert!(ready);
    }

    #[test]
    fn dispatch_not_ready_is_conflict() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_task(&mut s, "r1", "t2", vec!["t1".into()]);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t2.1".into(), task_id: "t2".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn dispatch_max_attempts_is_limit() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().attempts = 3;
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.4".into(), task_id: "t1".into(),
            attempt: 4, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Limit(_))));
    }

    #[test]
    fn dispatch_max_workers_is_limit() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        for i in 1..=5u32 {
            mk_task(&mut s, "r1", &format!("t{i}"), vec![]);
        }
        for i in 1..=4u32 {
            mk_dispatch(&mut s, "r1", &format!("t{i}"), 1);
        }
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t5.1".into(), task_id: "t5".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Limit(_))));
    }

    #[test]
    fn dispatch_second_running_for_same_task_is_conflict() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.2".into(), task_id: "t1".into(),
            attempt: 2, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn dispatch_retry_from_failed_allowed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "failed", 1);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.2".into(), task_id: "t1".into(),
            attempt: 2, retry_of: Some("r1.t1.1".into()), profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Ok(_, _)));
    }

    #[test]
    fn dispatch_attempt_mismatch_is_conflict() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.2".into(), task_id: "t1".into(),
            attempt: 2, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn dispatch_duplicate_id_is_conflict() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_task(&mut s, "r1", "t2", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "failed", 1);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(),
            id: "r1.t1.1".into(),
            task_id: "t1".into(),
            attempt: 2,
            retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn worker_done_succeeded_no_verify_completes_task() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Completed);
    }

    #[test]
    fn worker_done_succeeded_with_verify_sets_verifying() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "t".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: None, escalation: vec![],
            verify: vec!["bun test".into()], role: None, review: false, owner: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Verifying);
    }

    #[test]
    fn worker_done_failed_sets_task_failed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "failed", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Failed);
    }

    #[test]
    fn worker_done_duplicate_is_conflict() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 2, from: "r1.t1.1".into(),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn worker_done_older_dispatch_is_stale_no_state_change() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "failed", 1);
        mk_dispatch(&mut s, "r1", "t1", 2);
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 2, from: "r1.t1.1".into(),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Stale(_, _)));
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Dispatched);
    }

    fn set_dispatch_failed_no_outcome(s: &mut State, run_id: &str, dispatch_id: &str) {
        let d = s.runs.get_mut(run_id).unwrap().dispatches.get_mut(dispatch_id).unwrap();
        d.status = DispatchStatus::Failed;
        d.ended_at = Some(ts());
        if let Some(task) = s.runs.get_mut(run_id).unwrap().tasks.get_mut("t1") {
            task.status = TaskStatus::Failed;
        }
    }

    #[test]
    fn accepts_reports_running_dispatch() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        let d = &s.runs["r1"].dispatches["r1.t1.1"];
        assert!(d.accepts_reports());
    }

    #[test]
    fn accepts_reports_failed_dispatch_no_outcome() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        set_dispatch_failed_no_outcome(&mut s, "r1", "r1.t1.1");
        let d = &s.runs["r1"].dispatches["r1.t1.1"];
        assert!(d.accepts_reports(), "Failed+no outcome must accept reports");
    }

    #[test]
    fn accepts_reports_false_for_reported_dispatch() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        let d = &s.runs["r1"].dispatches["r1.t1.1"];
        assert!(!d.accepts_reports(), "Reported dispatch must not accept reports");
    }

    #[test]
    fn worker_done_revives_failed_dispatch_no_verify() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        set_dispatch_failed_no_outcome(&mut s, "r1", "r1.t1.1");
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 2, from: "r1.t1.1".into(),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            ApplyResult::Stale(_, _) => panic!("must not be stale for Failed+no-outcome"),
            ApplyResult::Err(e) => panic!("unexpected err: {}", e.message()),
        }
        assert_eq!(s.runs["r1"].dispatches["r1.t1.1"].status, DispatchStatus::Reported);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Completed);
    }

    #[test]
    fn worker_done_revives_failed_dispatch_with_verify() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "t".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: None, escalation: vec![],
            verify: vec!["bun test".into()], role: None, review: false, owner: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        mk_dispatch(&mut s, "r1", "t1", 1);
        set_dispatch_failed_no_outcome(&mut s, "r1", "r1.t1.1");
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 2, from: "r1.t1.1".into(),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            ApplyResult::Stale(_, _) => panic!("must not be stale"),
            ApplyResult::Err(e) => panic!("unexpected err: {}", e.message()),
        }
        assert_eq!(s.runs["r1"].dispatches["r1.t1.1"].status, DispatchStatus::Reported);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Verifying);
    }

    #[test]
    fn worker_done_failed_dispatch_with_outcome_already_set_rejected() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "failed", 1);
        let d = s.runs.get_mut("r1").unwrap().dispatches.get_mut("r1.t1.1").unwrap();
        assert!(d.outcome.is_some(), "outcome must be set");
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 2, from: "r1.t1.1".into(),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: ts(),
        };
        assert!(!matches!(s.apply(&cmd), ApplyResult::Ok(_, _)), "must be rejected when outcome already set");
    }

    #[test]
    fn worker_done_failed_dispatch_stale_when_task_not_failed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        let d = s.runs.get_mut("r1").unwrap().dispatches.get_mut("r1.t1.1").unwrap();
        d.status = DispatchStatus::Failed;
        d.ended_at = Some(ts());
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 2, from: "r1.t1.1".into(),
            to: vec!["coordinator".into()], msg_type: "worker_done".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("succeeded".into()), files_modified: vec![], summary: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Stale(_, _)), "must be stale when task not failed");
    }

    #[test]
    fn task_update_valid_transitions_accepted() {
        let pairs: &[(&str, TaskStatus, TaskStatus)] = &[
            ("t1", TaskStatus::Pending, TaskStatus::Cancelled),
            ("t2", TaskStatus::Pending, TaskStatus::Blocked),
            ("t3", TaskStatus::Blocked, TaskStatus::Pending),
            ("t4", TaskStatus::Failed, TaskStatus::Pending),
            ("t5", TaskStatus::Failed, TaskStatus::Cancelled),
        ];
        let mut s = State::default();
        mk_run(&mut s, "r1");
        for (tid, from, _to) in pairs {
            mk_task(&mut s, "r1", tid, vec![]);
            s.runs.get_mut("r1").unwrap().tasks.get_mut(*tid).unwrap().status = from.clone();
        }
        for (tid, _from, to) in pairs {
            let cmd = Cmd::TaskUpdate {
                run_id: "r1".into(), task_id: tid.to_string(),
                status: Some(to.clone()), result: None, spec: None,
                acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
            };
            assert!(matches!(s.apply(&cmd), ApplyResult::Ok(_, _)), "transition failed for {tid}");
        }
    }

    #[test]
    fn terminal_run_rejects_task_update() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        let set_done = Cmd::RunSetStatus {
            run_id: "r1".into(), status: RunStatus::Completed, now: ts(),
        };
        match s.apply(&set_done) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("set-status replay"),
            _ => panic!("cannot complete run"),
        }
        let cmd = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: None, result: Some("done".into()), spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn terminal_run_rejects_gate_create() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let cancel = Cmd::RunSetStatus {
            run_id: "r1".into(), status: RunStatus::Cancelled, now: ts(),
        };
        match s.apply(&cancel) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("cancel replay"),
            _ => panic!(),
        }
        let cmd = Cmd::GateCreate {
            run_id: "r1".into(), id: "g1".into(), task_id: "t1".into(),
            question: "q?".into(), options: vec![], msg_seq: 1, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn terminal_run_rejects_gate_resolve() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_gate(&mut s, "r1", "g1", "t1", 1);
        let cancel = Cmd::RunSetStatus {
            run_id: "r1".into(), status: RunStatus::Cancelled, now: ts(),
        };
        match s.apply(&cancel) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("cancel replay"),
            _ => panic!(),
        }
        let cmd = Cmd::GateResolve {
            run_id: "r1".into(), gate_id: "g1".into(),
            resolution: "yes".into(), now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn blocked_pending_with_open_gate_is_conflict() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_gate(&mut s, "r1", "g1", "t1", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Blocked);
        let cmd = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: Some(TaskStatus::Pending), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))));
    }

    #[test]
    fn two_open_gates_resolved_one_at_a_time() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_gate(&mut s, "r1", "g1", "t1", 1);
        mk_gate(&mut s, "r1", "g2", "t1", 2);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Blocked);

        let resolve1 = Cmd::GateResolve {
            run_id: "r1".into(), gate_id: "g1".into(),
            resolution: "yes".into(), now: ts(),
        };
        match s.apply(&resolve1) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("resolve1 replay"),
            _ => panic!("resolve g1 failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Blocked, "still blocked with g2 open");

        let resolve2 = Cmd::GateResolve {
            run_id: "r1".into(), gate_id: "g2".into(),
            resolution: "yes".into(), now: ts(),
        };
        match s.apply(&resolve2) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("resolve2 replay"),
            _ => panic!("resolve g2 failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Pending, "pending after both resolved");
    }

    #[test]
    fn task_update_replay_stops_all_running_dispatches() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);

        let d2 = crate::orch::model::Dispatch {
            id: "r1.t1.2".into(), task_id: "t1".into(), attempt: 2,
            retry_of: None, profile: None, model: None, tile_id: None,
            merge_branch: None,
            worktree: None, branch: None,
            status: DispatchStatus::Running, outcome: None, head_sha: None, files_modified: vec![],
            summary: None, report: None, started_at: ts(), ended_at: None, last_heartbeat: None,
            verifications: vec![], cost_usd: None, cost_source: None,
            account: None, chain_index: None, trigger: None, decision: None, failure_class: None, by: None, start_ref: None,
            native_home: None, native_session: None, corrective_retry: false, retry_same_entry: false, operational_wait: false,
        };
        s.runs.get_mut("r1").unwrap().dispatches.insert("r1.t1.2".into(), d2);

        let update = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: Some(TaskStatus::Failed), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
        };
        match s.apply(&update) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-update failed"),
        }

        let entry = &s.runs["r1"];
        let running = entry.dispatches.values().filter(|d| d.status == DispatchStatus::Running).count();
        assert_eq!(running, 0, "all running dispatches must be stopped");
    }

    #[test]
    fn replay_matches_apply_state() {
        let mut s = State::default();
        let mut events: Vec<(String, serde_json::Value)> = Vec::new();

        macro_rules! apply_capture {
            ($cmd:expr) => {
                match s.apply(&$cmd) {
                    ApplyResult::Ok(k, d) => {
                        events.push((k.clone(), d.clone()));
                        s.replay(&k, &d).expect("replay");
                    }
                    _ => panic!("apply failed"),
                }
            };
        }

        apply_capture!(Cmd::RunCreate {
            id: "r1".into(), title: "run".into(), repo: "/tmp/r".into(),
            base_ref: None, base_sha: None, setup: None,
            coordinator_tile: None, limits: Limits::default(), now: ts(),
        });
        apply_capture!(Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "task".into(),
            spec: "spec".into(), acceptance: None, deps: vec![],
            profile: None, escalation: vec![], verify: vec![], role: None, review: false, owner: None, now: ts(),
        });
        apply_capture!(Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        });

        let mut fresh = State::default();
        for (k, d) in &events {
            fresh.replay(k, d).expect("fresh replay");
        }

        assert_eq!(
            serde_json::to_value(&s.runs["r1"].run).unwrap(),
            serde_json::to_value(&fresh.runs["r1"].run).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&s.runs["r1"].tasks["t1"]).unwrap(),
            serde_json::to_value(&fresh.runs["r1"].tasks["t1"]).unwrap()
        );
    }

    #[test]
    fn concurrent_dispatch_serializes_correctly() {
        use std::sync::{Arc, Mutex};

        let state = Arc::new(Mutex::new(State::default()));
        {
            let mut guard = state.lock().unwrap();
            mk_run(&mut guard, "r1");
            for i in 1..=4u32 {
                mk_task(&mut guard, "r1", &format!("t{i}"), vec![]);
            }
            guard.runs.get_mut("r1").unwrap().run.limits.max_workers = 2;
        }

        let results: Vec<_> = (0..8)
            .map(|i| {
                let state = state.clone();
                std::thread::spawn(move || -> Option<(String, u32)> {
                    let task_id = format!("t{}", (i % 4) + 1);
                    let mut guard = state.lock().unwrap();
                    let entry = guard.runs.get("r1").unwrap();
                    let task = entry.tasks.get(&task_id)?;
                    let attempt = task.attempts + 1;
                    let dispatch_id = format!("r1.{task_id}.{attempt}");
                    let cmd = Cmd::Dispatch {
                        run_id: "r1".into(), id: dispatch_id, task_id: task_id.clone(),
                        attempt, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
                    };
                    match guard.apply(&cmd) {
                        ApplyResult::Ok(k, d) => {
                            guard.replay(&k, &d).expect("concurrent replay");
                            Some((task_id, attempt))
                        }
                        _ => None,
                    }
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect();

        let guard = state.lock().unwrap();
        let running = guard.runs["r1"].running_dispatch_count();
        assert!(running <= 2, "running {running} > maxWorkers 2");

        // Per-task uniqueness: no two successful dispatches for the same task with the same attempt
        let successful: Vec<(String, u32)> = results.into_iter().flatten().collect();
        let mut per_task: std::collections::HashMap<String, std::collections::HashSet<u32>> =
            std::collections::HashMap::new();
        for (tid, attempt) in &successful {
            let inserted = per_task.entry(tid.clone()).or_default().insert(*attempt);
            assert!(inserted, "duplicate attempt {attempt} for task {tid}");
        }
    }

    #[test]
    fn zero_task_run_can_be_completed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::RunSetStatus {
            run_id: "r1".into(), status: RunStatus::Completed, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Ok(_, _)));
    }

    #[test]
    fn run_set_status_cancel_stops_all_dispatches() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_task(&mut s, "r1", "t2", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);
        mk_dispatch(&mut s, "r1", "t2", 1);
        let cmd = Cmd::RunSetStatus {
            run_id: "r1".into(), status: RunStatus::Cancelled, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("cancel replay"),
            _ => panic!(),
        }
        let running = s.runs["r1"].running_dispatch_count();
        assert_eq!(running, 0);
    }

    #[test]
    fn send_message_to_multiple_recipients() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 1,
            from: "coordinator".into(), to: vec!["a".into(), "b".into(), "c".into()],
            msg_type: "note".into(), task_id: None, dispatch_id: None,
            reply_to: None, body: None, outcome: None, files_modified: vec![], summary: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        assert_eq!(s.runs["r1"].messages.len(), 3);
        assert_eq!(s.runs["r1"].msg_seq, 3);
    }

    #[test]
    fn ack_marks_messages_read() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 1, from: "coordinator".into(),
            to: vec!["worker1".into()], msg_type: "note".into(),
            task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: None, files_modified: vec![], summary: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        assert!(!s.runs["r1"].messages[0].acked);
        let ack = Cmd::Ack { run_id: "r1".into(), seqs: vec![1] };
        match s.apply(&ack) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        assert!(s.runs["r1"].messages[0].acked);
    }

    #[test]
    fn gate_create_blocks_task() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Pending);
        mk_gate(&mut s, "r1", "g1", "t1", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Blocked);
    }

    #[test]
    fn gate_resolve_unblocks_task_when_all_gates_resolved() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_gate(&mut s, "r1", "g1", "t1", 1);
        let cmd = Cmd::GateResolve {
            run_id: "r1".into(), gate_id: "g1".into(), resolution: "yes".into(), now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Pending);
    }

    #[test]
    fn max_workers_enforced_across_tasks() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        s.runs.get_mut("r1").unwrap().run.limits.max_workers = 2;
        for i in 1..=4u32 {
            mk_task(&mut s, "r1", &format!("t{i}"), vec![]);
        }
        mk_dispatch(&mut s, "r1", "t1", 1);
        mk_dispatch(&mut s, "r1", "t2", 1);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t3.1".into(), task_id: "t3".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Limit(_))));
    }

    fn mk_run_with_base(state: &mut State, id: &str, base_sha: &str) {
        let cmd = Cmd::RunCreate {
            id: id.to_string(),
            title: "run".into(),
            repo: "/tmp/r".into(),
            base_ref: Some("HEAD".into()),
            base_sha: Some(base_sha.into()),
            setup: None,
            coordinator_tile: None,
            limits: Limits::default(),
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("mk_run_with_base replay"),
            _ => panic!("run-create with base_sha failed"),
        }
    }

    fn mk_dispatch_with_worktree(state: &mut State, run_id: &str, task_id: &str, attempt: u32, worktree: &str, branch: &str) -> String {
        let dispatch_id = mk_dispatch(state, run_id, task_id, attempt);
        let cmd = Cmd::DispatchSetWorktree {
            run_id: run_id.into(),
            dispatch_id: dispatch_id.clone(),
            worktree: worktree.into(),
            branch: branch.into(),
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("set-worktree replay"),
            _ => panic!("dispatch-set-worktree failed"),
        }
        dispatch_id
    }

    #[test]
    fn dispatch_set_worktree_stores_path_and_branch() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch_with_worktree(&mut s, "r1", "t1", 1, "/repo/.claude/worktrees/orch-r1-t1-1", "orch/r1/t1-1");
        let d = &s.runs["r1"].dispatches[&did];
        assert_eq!(d.worktree.as_deref(), Some("/repo/.claude/worktrees/orch-r1-t1-1"));
        assert_eq!(d.branch.as_deref(), Some("orch/r1/t1-1"));
    }

    #[test]
    fn dispatch_record_head_sets_sha_and_files() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", &did, "succeeded", 1);

        let cmd = Cmd::DispatchRecordHead {
            run_id: "r1".into(),
            dispatch_id: did.clone(),
            head_sha: "abc123".into(),
            files_modified: vec!["foo.rs".into()],
            now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("dispatch-record-head failed"),
        }
        let d = &s.runs["r1"].dispatches[&did];
        assert_eq!(d.head_sha.as_deref(), Some("abc123"));
        assert_eq!(d.files_modified, vec!["foo.rs"]);
    }

    #[test]
    fn task_integrate_requires_completed_status() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        mk_dispatch(&mut s, "r1", "t1", 1);

        let cmd = Cmd::TaskIntegrate {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            integrated_sha: "sha1".into(), msg_seq: 1, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Err(OrchError::Conflict(_))), "dispatched task should not integrate");
    }

    #[test]
    fn task_integrate_stores_sha_and_sends_message() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        {
            let entry = s.runs.get_mut("r1").unwrap();
            if let Some(d) = entry.dispatches.get_mut(&did) {
                d.branch = Some("orch/r1/t1-1".into());
            }
        }
        worker_done(&mut s, "r1", &did, "succeeded", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Completed);

        let cmd = Cmd::TaskIntegrate {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: did.clone(),
            integrated_sha: "sha1".into(), msg_seq: 2, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-integrate failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].integrated_sha.as_deref(), Some("sha1"));
        let msgs = &s.runs["r1"].messages;
        assert!(msgs.iter().any(|m| m.msg_type == "task_integrated" && m.task_id.as_deref() == Some("t1")));
    }

    #[test]
    fn task_conflict_blocks_task_and_sends_message() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        {
            let entry = s.runs.get_mut("r1").unwrap();
            if let Some(d) = entry.dispatches.get_mut(&did) {
                d.branch = Some("orch/r1/t1-1".into());
            }
        }
        worker_done(&mut s, "r1", &did, "succeeded", 1);

        let cmd = Cmd::TaskConflict {
            run_id: "r1".into(), task_id: "t1".into(),
            files: vec!["a.rs".into(), "b.rs".into()],
            branch: "orch/r1/t1-1".into(), integration_tip: "tip1".into(), msg_seq: 2, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-conflict failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Blocked);
        let result = s.runs["r1"].tasks["t1"].result.as_deref().unwrap_or("");
        assert!(result.contains("a.rs"), "conflict result should list files");
        let msgs = &s.runs["r1"].messages;
        assert!(msgs.iter().any(|m| m.msg_type == "integration_conflict" && m.task_id.as_deref() == Some("t1")));
    }

    #[test]
    fn conflict_resolve_dispatch_completes_blocked_user_owned_task() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().owner = Some("user".into());
        s.runs.get_mut("r1").unwrap().dispatches.get_mut(&did).unwrap().branch = Some("orch/r1/t1-1".into());
        worker_done(&mut s, "r1", &did, "succeeded", 1);
        let conflict = Cmd::TaskConflict {
            run_id: "r1".into(), task_id: "t1".into(), files: vec!["a.rs".into()],
            branch: "orch/r1/t1-1".into(), integration_tip: "tip1".into(), msg_seq: 2, now: ts(),
        };
        match s.apply(&conflict) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-conflict failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Blocked);
        let body = s.runs["r1"].messages.iter()
            .find(|m| m.msg_type == "integration_conflict").and_then(|m| m.body.clone()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(parsed["files"][0], "a.rs");
        assert_eq!(parsed["branch"], "orch/r1/t1-1");
        assert_eq!(parsed["integrationTip"], "tip1");
        assert!(parsed["hint"].as_str().unwrap().contains("orch conflict-resolve"));

        let plain = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.2".into(), task_id: "t1".into(), attempt: 2,
            retry_of: None, profile: None, model: None, tile_id: None, owner: None,
            merge_branch: None, now: ts(),
        };
        assert!(matches!(s.apply(&plain), ApplyResult::Err(OrchError::Conflict(_))), "plain dispatch must not unblock");

        let resolve = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.2".into(), task_id: "t1".into(), attempt: 2,
            retry_of: None, profile: None, model: None, tile_id: None, owner: None,
            merge_branch: Some("orch/r1/t1-1".into()), now: ts(),
        };
        match s.apply(&resolve) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("resolve dispatch failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Dispatched);
        assert_eq!(s.runs["r1"].dispatches["r1.t1.2"].merge_branch.as_deref(), Some("orch/r1/t1-1"));
        assert!(s.runs["r1"].dispatches.contains_key("r1.t1.1"), "original attempt stays recorded");

        s.runs.get_mut("r1").unwrap().dispatches.get_mut("r1.t1.2").unwrap().branch = Some("orch/r1/t1-2".into());
        worker_done(&mut s, "r1", "r1.t1.2", "succeeded", 3);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Completed);

        let integrate = Cmd::TaskIntegrate {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.2".into(),
            integrated_sha: "sha2".into(), msg_seq: 4, now: ts(),
        };
        match s.apply(&integrate) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-integrate failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].integrated_sha.as_deref(), Some("sha2"));
        let integrated = s.runs["r1"].messages.iter()
            .find(|m| m.msg_type == "task_integrated").and_then(|m| m.body.clone()).unwrap();
        assert!(integrated.contains("sha2"));
        assert!(integrated.contains("Merged into orch/r1/integration (your main branch is untouched)"));
    }

    #[test]
    fn run_create_stores_base_sha_and_setup() {
        let mut s = State::default();
        mk_run_with_base(&mut s, "r1", "deadbeef");
        assert_eq!(s.runs["r1"].run.base_sha.as_deref(), Some("deadbeef"));
    }

    fn mk_task_with_verify(state: &mut State, run_id: &str, task_id: &str, review: bool) {
        let cmd = Cmd::TaskCreate {
            run_id: run_id.into(),
            id: task_id.into(),
            title: "task".into(),
            spec: "spec".into(),
            acceptance: None,
            deps: vec![],
            profile: None,
            escalation: vec![],
            verify: vec!["bun test".into()],
            role: None,
            review,
            owner: None,
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("mk_task_with_verify replay"),
            _ => panic!("task-create failed"),
        }
    }

    fn set_task_verifying(state: &mut State, run_id: &str, task_id: &str) {
        let entry = state.runs.get_mut(run_id).unwrap();
        let task = entry.tasks.get_mut(task_id).unwrap();
        task.status = TaskStatus::Verifying;
    }

    #[test]
    fn task_verified_no_review_completes_task() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", false);
        set_task_verifying(&mut s, "r1", "t1");
        let cmd = Cmd::TaskVerified {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            msg_seq: 1, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-verified failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Completed);
    }

    #[test]
    fn task_verified_with_review_sets_review() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", true);
        set_task_verifying(&mut s, "r1", "t1");
        let cmd = Cmd::TaskVerified {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            msg_seq: 1, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-verified failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Review);
    }

    #[test]
    fn task_verified_sends_verification_passed_message() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", false);
        set_task_verifying(&mut s, "r1", "t1");
        let cmd = Cmd::TaskVerified {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            msg_seq: 1, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        let msgs = &s.runs["r1"].messages;
        assert!(msgs.iter().any(|m| m.msg_type == "verification_passed" && m.task_id.as_deref() == Some("t1")));
    }

    #[test]
    fn task_verification_failed_sets_task_failed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", false);
        set_task_verifying(&mut s, "r1", "t1");
        let cmd = Cmd::TaskVerificationFailed {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            command: "bun test".into(), exit_code: Some(1), timed_out: false,
            tail: "FAIL: 1 test failed".into(), msg_seq: 1, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-verification-failed failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Failed);
    }

    #[test]
    fn task_verification_failed_sends_message() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", false);
        set_task_verifying(&mut s, "r1", "t1");
        let cmd = Cmd::TaskVerificationFailed {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            command: "bun test".into(), exit_code: Some(2), timed_out: false,
            tail: "tail output".into(), msg_seq: 1, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        let msgs = &s.runs["r1"].messages;
        assert!(msgs.iter().any(|m| m.msg_type == "verification_failed" && m.task_id.as_deref() == Some("t1")));
    }

    #[test]
    fn dispatch_append_verification_stores_result() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        let cmd = Cmd::DispatchAppendVerification {
            run_id: "r1".into(), dispatch_id: did.clone(),
            command: "bun test".into(), exit_code: Some(0), timed_out: false,
            tail: vec!["all pass".into()], head_sha: Some("abc".into()), now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("dispatch-append-verification failed"),
        }
        let d = &s.runs["r1"].dispatches[&did];
        assert_eq!(d.verifications.len(), 1);
        assert_eq!(d.verifications[0].command, "bun test");
        assert_eq!(d.verifications[0].exit_code, Some(0));
        assert!(!d.verifications[0].timed_out);
    }

    #[test]
    fn dispatch_append_verification_timeout_recorded() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        let cmd = Cmd::DispatchAppendVerification {
            run_id: "r1".into(), dispatch_id: did.clone(),
            command: "bun test".into(), exit_code: None, timed_out: true,
            tail: vec![], head_sha: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        let d = &s.runs["r1"].dispatches[&did];
        assert!(d.verifications[0].timed_out);
        assert_eq!(d.verifications[0].exit_code, None);
    }

    #[test]
    fn dispatch_record_cost_stores_exact() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        let cmd = Cmd::DispatchRecordCost {
            run_id: "r1".into(), dispatch_id: did.clone(),
            cost_usd: Some(0.042), cost_source: "exact".into(), now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("dispatch-record-cost failed"),
        }
        let d = &s.runs["r1"].dispatches[&did];
        assert_eq!(d.cost_source.as_deref(), Some("exact"));
        assert!((d.cost_usd.unwrap() - 0.042).abs() < 1e-9);
    }

    #[test]
    fn dispatch_record_cost_stores_unknown() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let did = mk_dispatch(&mut s, "r1", "t1", 1);
        let cmd = Cmd::DispatchRecordCost {
            run_id: "r1".into(), dispatch_id: did.clone(),
            cost_usd: None, cost_source: "unknown".into(), now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!(),
        }
        let d = &s.runs["r1"].dispatches[&did];
        assert_eq!(d.cost_source.as_deref(), Some("unknown"));
        assert!(d.cost_usd.is_none());
    }

    #[test]
    fn task_re_verify_transitions_failed_to_verifying() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", false);
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Failed;
        let cmd = Cmd::TaskReVerify { run_id: "r1".into(), task_id: "t1".into(), now: ts() };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-re-verify failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Verifying);
    }

    #[test]
    fn verification_replay_is_deterministic() {
        let mut s = State::default();
        let mut events: Vec<(String, serde_json::Value)> = Vec::new();

        macro_rules! apply_ev {
            ($cmd:expr) => {
                match s.apply(&$cmd) {
                    ApplyResult::Ok(k, d) => {
                        events.push((k.clone(), d.clone()));
                        s.replay(&k, &d).expect("replay");
                    }
                    _ => panic!("apply failed"),
                }
            };
        }

        apply_ev!(Cmd::RunCreate {
            id: "r1".into(), title: "r".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None,
            coordinator_tile: None, limits: Limits::default(), now: ts(),
        });
        apply_ev!(Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "t".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: None, escalation: vec![],
            verify: vec!["check".into()], role: None, review: false, owner: None, now: ts(),
        });
        apply_ev!(Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, merge_branch: None, now: ts(),
        });
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Verifying;
        apply_ev!(Cmd::DispatchAppendVerification {
            run_id: "r1".into(), dispatch_id: "r1.t1.1".into(),
            command: "check".into(), exit_code: Some(0), timed_out: false,
            tail: vec!["ok".into()], head_sha: None, now: ts(),
        });
        apply_ev!(Cmd::DispatchRecordCost {
            run_id: "r1".into(), dispatch_id: "r1.t1.1".into(),
            cost_usd: Some(0.1), cost_source: "exact".into(), now: ts(),
        });
        apply_ev!(Cmd::TaskVerified {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            msg_seq: 1, now: ts(),
        });

        let mut fresh = State::default();
        for (k, d) in &events {
            fresh.replay(k, d).expect("fresh replay");
        }

        assert_eq!(
            serde_json::to_value(&s.runs["r1"].tasks["t1"]).unwrap(),
            serde_json::to_value(&fresh.runs["r1"].tasks["t1"]).unwrap()
        );
        assert_eq!(
            s.runs["r1"].dispatches["r1.t1.1"].verifications.len(),
            fresh.runs["r1"].dispatches["r1.t1.1"].verifications.len()
        );
        assert_eq!(
            s.runs["r1"].dispatches["r1.t1.1"].cost_usd,
            fresh.runs["r1"].dispatches["r1.t1.1"].cost_usd
        );
    }

    #[test]
    fn limits_serde_defaults_for_new_fields() {
        let json = r#"{"maxWorkers":2,"maxAttempts":3,"dispatchTimeoutMin":30}"#;
        let limits: Limits = serde_json::from_str(json).unwrap();
        assert_eq!(limits.verify_timeout_min, 15);
        assert_eq!(limits.stall_quiet_min, 5);
    }

    fn mk_task_review_only(state: &mut State, run_id: &str, task_id: &str) {
        let cmd = Cmd::TaskCreate {
            run_id: run_id.into(),
            id: task_id.into(),
            title: "task".into(),
            spec: "spec".into(),
            acceptance: None,
            deps: vec![],
            profile: None,
            escalation: vec![],
            verify: vec![],
            role: None,
            review: true,
            owner: None,
            now: ts(),
        };
        match state.apply(&cmd) {
            ApplyResult::Ok(k, d) => state.replay(&k, &d).expect("replay"),
            _ => panic!("task-create failed"),
        }
    }

    #[test]
    fn worker_done_succeeded_no_verify_review_true_sets_review() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_review_only(&mut s, "r1", "t1");
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Review);
    }

    #[test]
    fn worker_done_succeeded_verify_and_review_sets_verifying() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_with_verify(&mut s, "r1", "t1", true);
        mk_dispatch(&mut s, "r1", "t1", 1);
        worker_done(&mut s, "r1", "r1.t1.1", "succeeded", 1);
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Verifying);
    }

    #[test]
    fn review_to_completed_via_task_update() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_review_only(&mut s, "r1", "t1");
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Review;
        let cmd = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: Some(TaskStatus::Completed), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-update failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Completed);
    }

    #[test]
    fn review_to_pending_with_spec_edit_atomic() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_review_only(&mut s, "r1", "t1");
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Review;
        let cmd = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: Some(TaskStatus::Pending), result: None, spec: Some("new spec".into()),
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-update failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Pending);
        assert_eq!(s.runs["r1"].tasks["t1"].spec, "new spec");
    }

    #[test]
    fn review_to_cancelled() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task_review_only(&mut s, "r1", "t1");
        s.runs.get_mut("r1").unwrap().tasks.get_mut("t1").unwrap().status = TaskStatus::Review;
        let cmd = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: Some(TaskStatus::Cancelled), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, owner: None, by: "user".into(), reason: None, now: ts(),
        };
        assert!(matches!(s.apply(&cmd), ApplyResult::Ok(_, _)));
    }

    #[test]
    fn owner_user_set_via_dispatch_event() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_task(&mut s, "r1", "t1", vec![]);
        let cmd = Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None,
            owner: Some("user".into()), merge_branch: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("dispatch failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].owner.as_deref(), Some("user"));
    }

    #[test]
    fn ownership_change_requires_reason_and_replays() {
        let mut state = State::default();
        mk_run(&mut state, "r1");
        mk_task(&mut state, "r1", "t1", vec![]);
        let mut cmd = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(), status: None, result: None,
            spec: None, acceptance: None, verify: None, profile: None, escalation: None,
            review: None, owner: Some("coordinator".into()), by: "coordinator".into(),
            reason: None, now: ts(),
        };
        assert!(matches!(state.apply(&cmd), ApplyResult::Err(_)));
        if let Cmd::TaskUpdate { reason, .. } = &mut cmd { *reason = Some("user asked in chat".into()); }
        let (kind, data) = match state.apply(&cmd) {
            ApplyResult::Ok(kind, data) => (kind, data),
            _ => panic!("ownership update failed"),
        };
        assert_eq!(data["by"], "coordinator");
        state.replay(&kind, &data).unwrap();
        assert_eq!(state.runs["r1"].tasks["t1"].owner.as_deref(), Some("coordinator"));
        assert_eq!(state.runs["r1"].tasks["t1"].owner_reason.as_deref(), Some("user asked in chat"));
    }

    #[test]
    fn run_update_changes_title() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::RunUpdate { run_id: "r1".into(), title: "new title".into(), now: ts() };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("run-update failed"),
        }
        assert_eq!(s.runs["r1"].run.title, "new title");
    }

    #[test]
    fn run_set_coordinator_replay() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::RunSetCoordinator {
            run_id: "r1".into(), tile_id: Some("tile-abc".into()), now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("run-set-coordinator failed"),
        }
        assert_eq!(s.runs["r1"].run.coordinator_tile.as_deref(), Some("tile-abc"));
        let cmd2 = Cmd::RunSetCoordinator {
            run_id: "r1".into(), tile_id: None, now: ts(),
        };
        match s.apply(&cmd2) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("run-set-coordinator detach failed"),
        }
        assert_eq!(s.runs["r1"].run.coordinator_tile, None);
    }

    #[test]
    fn task_create_with_role_applies_and_replays() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "explore task".into(),
            spec: "look around".into(), acceptance: None, deps: vec![],
            profile: None, escalation: vec!["claude:haiku".into()],
            verify: vec![], role: Some(crate::orch::model::Role::Explore),
            review: false, owner: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-create failed"),
        }
        let task = &s.runs["r1"].tasks["t1"];
        assert_eq!(task.role, Some(crate::orch::model::Role::Explore));
    }

    #[test]
    fn task_create_without_role_replays_as_none() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "task".into(),
            spec: "spec".into(), acceptance: None, deps: vec![],
            profile: None, escalation: vec![], verify: vec![], role: None,
            review: false, owner: None, now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("task-create failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].role, None);
    }

    #[test]
    fn task_create_old_log_without_role_replays_ok() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let data = serde_json::json!({
            "runId": "r1", "id": "t1", "title": "old task", "spec": "spec",
            "acceptance": null, "deps": [], "profile": null, "escalation": [],
            "verify": [], "review": false, "owner": null, "now": ts()
        });
        s.replay("task-create", &data).expect("old log replay should not fail");
        assert_eq!(s.runs["r1"].tasks["t1"].role, None,
            "task from old log without role field should have role=None");
    }

    #[test]
    fn run_coordinator_cost_freezes_value() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::RunCoordinatorCost { run_id: "r1".into(), cost_usd: 0.42, now: ts() };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("run-coordinator-cost failed"),
        }
        assert_eq!(s.runs["r1"].coordinator_cost_final, Some(0.42));
    }

    #[test]
    fn run_coordinator_baseline_recorded() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::RunRecordCoordinatorBaseline { run_id: "r1".into(), cost_usd: Some(1.0), now: ts() };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("run-coordinator-baseline failed"),
        }
        assert_eq!(s.runs["r1"].coordinator_cost_baseline, Some(1.0));
    }

    fn commit_cmd(s: &mut State, cmd: Cmd) -> Result<(String, Value), OrchError> {
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) | ApplyResult::Stale(k, d) => {
                s.replay(&k, &d).expect("replay");
                Ok((k, d))
            }
            ApplyResult::Err(e) => Err(e),
        }
    }

    #[test]
    fn run_idle_and_run_active_replay_and_reject_duplicate_transitions() {
        let mut state = State::default();
        let create = serde_json::json!({"id": "r1", "detached": true});
        state.replay("run-create", &create).unwrap();
        let mut log: Vec<(String, Value)> = vec![("run-create".into(), create)];
        let mut push = |s: &mut State, cmd: Cmd| {
            let ev = commit_cmd(s, cmd).unwrap();
            log.push(ev);
        };
        assert_eq!(state.runs["r1"].idle_state, IdleState::Active);

        assert!(matches!(
            commit_cmd(&mut state, Cmd::RunActive { run_id: "r1".into(), now: ts() }),
            Err(OrchError::Conflict(_))
        ));

        let seq1 = state.runs["r1"].next_msg_seq();
        push(&mut state, Cmd::RunIdle {
            run_id: "r1".into(), msg_seq: seq1, msg_type: "run_idle".into(),
            body: r#"{"reason":"idle"}"#.into(), now: ts(),
        });
        assert_eq!(state.runs["r1"].idle_state, IdleState::Idle);
        assert_eq!(state.runs["r1"].messages.len(), 1);
        assert_eq!(state.runs["r1"].messages[0].seq, seq1);
        assert_eq!(state.runs["r1"].messages[0].msg_type, "run_idle");
        assert_eq!(state.runs["r1"].messages[0].to, "coordinator");

        let dup_seq = state.runs["r1"].next_msg_seq();
        assert!(matches!(
            commit_cmd(&mut state, Cmd::RunIdle {
                run_id: "r1".into(), msg_seq: dup_seq,
                msg_type: "run_idle".into(), body: r#"{"reason":"idle"}"#.into(), now: ts(),
            }),
            Err(OrchError::Conflict(_))
        ));

        push(&mut state, Cmd::RunActive { run_id: "r1".into(), now: ts() });
        assert_eq!(state.runs["r1"].idle_state, IdleState::Active);

        let seq2 = state.runs["r1"].next_msg_seq();
        push(&mut state, Cmd::RunIdle {
            run_id: "r1".into(), msg_seq: seq2, msg_type: "run_finished".into(),
            body: r#"{"completed":1,"integrated":1,"cancelled":0}"#.into(), now: ts(),
        });
        assert_eq!(state.runs["r1"].idle_state, IdleState::Finished);
        assert_eq!(state.runs["r1"].messages.len(), 2);
        assert_eq!(state.runs["r1"].messages[1].seq, seq2);
        assert_eq!(state.runs["r1"].messages[1].msg_type, "run_finished");

        let dup_seq2 = state.runs["r1"].next_msg_seq();
        assert!(matches!(
            commit_cmd(&mut state, Cmd::RunIdle {
                run_id: "r1".into(), msg_seq: dup_seq2,
                msg_type: "run_finished".into(), body: r#"{"completed":1,"integrated":1,"cancelled":0}"#.into(), now: ts(),
            }),
            Err(OrchError::Conflict(_))
        ));

        let mut replayed = State::default();
        for (kind, data) in &log {
            replayed.replay(kind, data).unwrap();
        }
        assert_eq!(replayed.runs["r1"].idle_state, state.runs["r1"].idle_state);
        assert_eq!(replayed.runs["r1"].msg_seq, state.runs["r1"].msg_seq);
        assert_eq!(replayed.runs["r1"].messages.len(), state.runs["r1"].messages.len());
        for (a, b) in replayed.runs["r1"].messages.iter().zip(state.runs["r1"].messages.iter()) {
            assert_eq!((a.seq, &a.msg_type, &a.body, &a.from, &a.to), (b.seq, &b.msg_type, &b.body, &b.from, &b.to));
        }
    }

    #[test]
    fn changed_idle_fingerprint_emits_and_replays() {
        let mut state = State::default();
        let create = serde_json::json!({"id": "r-fingerprint", "detached": true});
        state.replay("run-create", &create).unwrap();
        let idle = |body: &str, seq| Cmd::RunIdle {
            run_id: "r-fingerprint".into(), msg_seq: seq, msg_type: "run_idle".into(),
            body: body.into(), now: 1,
        };
        let pending = r#"{"tasks":{"failed":[{"id":"t1","detail":"first"}]}}"#;
        let blocked = r#"{"tasks":{"failed":[{"id":"t1","detail":"second"}]}}"#;
        let first = commit_cmd(&mut state, idle(pending, 1)).unwrap();
        assert!(commit_cmd(&mut state, idle(pending, 2)).is_err());
        let second = commit_cmd(&mut state, idle(blocked, 2)).unwrap();
        let mut replayed = State::default();
        replayed.replay("run-create", &create).unwrap();
        for (kind, data) in [first, second] { replayed.replay(&kind, &data).unwrap(); }
        assert_eq!(replayed.runs["r-fingerprint"].messages.len(), 2);
        assert!(commit_cmd(&mut replayed, idle(blocked, 3)).is_err());
    }

    #[test]
    fn task_record_failure_emits_no_task_exhausted_and_replays_legacy_payload() {
        for detached in [false, true] {
            let mut state = State::default();
            state.replay("run-create", &serde_json::json!({
                "id": "r1", "detached": detached,
                "limits": Limits { max_attempts: 2, ..Limits::default() },
            })).unwrap();
            mk_routed_task(&mut state, "r1", "t1");
            record_on_new_dispatch(&mut state, 1, FailureClass::QualityFailed, Some(0));
            let dispatch_id = mk_dispatch(&mut state, "r1", "t1", 2);
            commit_cmd(&mut state, Cmd::DispatchUpdate {
                run_id: "r1".into(), dispatch_id: dispatch_id.clone(), status: DispatchStatus::Failed,
                summary: Some("test failure\nlog follow-up".into()), now: ts(),
            }).unwrap();
            let (kind, data) = commit_cmd(&mut state, Cmd::TaskRecordFailure {
                run_id: "r1".into(), task_id: "t1".into(), dispatch_id,
                class: FailureClass::QualityFailed, chain_index: Some(1), now: ts(),
            }).unwrap();
            assert!(data.get("taskExhausted").is_none());
            assert_eq!(state.runs["r1"].tasks["t1"].quality_attempts, 2);
            assert!(state.runs["r1"].messages.is_empty());
            state.replay(&kind, &data).unwrap();
            assert!(state.runs["r1"].messages.is_empty());
        }

        let mut legacy = State::default();
        legacy.replay("run-create", &serde_json::json!({"id": "r1", "detached": true})).unwrap();
        mk_routed_task(&mut legacy, "r1", "t1");
        let dispatch_id = mk_dispatch(&mut legacy, "r1", "t1", 1);
        legacy.replay("task-record-failure", &serde_json::json!({
            "runId": "r1", "taskId": "t1", "dispatchId": dispatch_id,
            "class": "quality_failed", "chainIndex": 0, "now": ts(),
            "taskExhausted": {"taskId": "t1", "title": "t", "summary": "failed"},
        })).unwrap();
        assert_eq!(legacy.runs["r1"].tasks["t1"].quality_attempts, 1);
        assert!(legacy.runs["r1"].messages.is_empty());
    }

    #[test]
    fn old_events_replay_without_detached_fields() {
        let mut state = State::default();
        state.replay("run-create", &serde_json::json!({"id": "r1", "title": "old"})).unwrap();
        state.replay("run-update", &serde_json::json!({"runId": "r1", "title": "updated"})).unwrap();
        mk_routed_task(&mut state, "r1", "t1");
        let dispatch_id = mk_dispatch(&mut state, "r1", "t1", 1);
        state.replay("task-record-failure", &serde_json::json!({
            "runId": "r1", "taskId": "t1", "dispatchId": dispatch_id, "class": "quality_failed",
        })).unwrap();
        let entry = &state.runs["r1"];
        assert_eq!(entry.run.title, "updated");
        assert!(!entry.run.detached);
        assert_eq!(entry.run.autonomy, Autonomy::default());
        assert_eq!(entry.tasks["t1"].quality_attempts, 1);
        assert!(entry.messages.is_empty());
    }

    fn mk_routed_task(s: &mut State, run_id: &str, task_id: &str) {
        let cmd = Cmd::TaskCreate {
            run_id: run_id.into(), id: task_id.into(), title: "t".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: None, escalation: vec![], verify: vec![],
            role: None, review: false, owner: None, now: ts(),
        }.with_fields(serde_json::json!({"routing": 1, "pinned": false}));
        commit_cmd(s, cmd).expect("routed task-create");
    }

    fn dispatch_cmd(run_id: &str, task_id: &str, attempt: u32) -> Cmd {
        Cmd::Dispatch { merge_branch: None,
            run_id: run_id.into(), id: format!("{run_id}.{task_id}.{attempt}"), task_id: task_id.into(),
            attempt, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
        }
    }

    fn fail_dispatch(s: &mut State, run_id: &str, dispatch_id: &str) {
        let cmd = Cmd::DispatchUpdate {
            run_id: run_id.into(), dispatch_id: dispatch_id.into(),
            status: DispatchStatus::Failed, summary: None, now: ts(),
        };
        commit_cmd(s, cmd).expect("dispatch-update");
    }

    fn record_failure(s: &mut State, dispatch_id: &str, class: FailureClass, chain_index: Option<u32>) {
        let cmd = Cmd::TaskRecordFailure {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: dispatch_id.into(),
            class, chain_index, now: ts(),
        };
        commit_cmd(s, cmd).expect("task-record-failure");
    }

    fn record_on_new_dispatch(s: &mut State, attempt: u32, class: FailureClass, chain_index: Option<u32>) {
        let id = mk_dispatch(s, "r1", "t1", attempt);
        fail_dispatch(s, "r1", &id);
        record_failure(s, &id, class, chain_index);
    }

    fn wait_request(resume_at: Option<i64>) -> WaitRequest {
        WaitRequest {
            kind: WaitKind::Quota, resume_at, reason: "quota".into(),
            candidate: Some(crate::orch::candidates::CandidateKey::from_chain_entry("claude:opus")),
            same_session: false,
        }
    }

    fn wait_cmd(resume_at: Option<i64>) -> Cmd {
        Cmd::TaskWait { run_id: "r1".into(), task_id: "t1".into(), wait: wait_request(resume_at), now: ts() }
    }

    fn resume_cmd(resume_id: &str) -> Cmd {
        Cmd::TaskResume { run_id: "r1".into(), task_id: "t1".into(), resume_id: resume_id.into(), now: ts() }
    }

    fn routed_failed_task() -> State {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_routed_task(&mut s, "r1", "t1");
        let d = mk_dispatch(&mut s, "r1", "t1", 1);
        fail_dispatch(&mut s, "r1", &d);
        s
    }

    #[test]
    fn f12_old_log_without_new_fields_replays_as_legacy() {
        let mut s = State::default();
        let events = [
            ("run-create", serde_json::json!({"id":"r1","title":"t","repo":"/x","limits":{"maxWorkers":2,"maxAttempts":2,"dispatchTimeoutMin":60},"now":1})),
            ("task-create", serde_json::json!({"runId":"r1","id":"t1","title":"t","spec":"s","deps":[],"escalation":[],"verify":[],"review":false,"now":1})),
            ("dispatch", serde_json::json!({"runId":"r1","id":"r1.t1.1","taskId":"t1","attempt":1,"now":2})),
            ("dispatch-update", serde_json::json!({"runId":"r1","dispatchId":"r1.t1.1","status":"failed","now":3})),
        ];
        for (k, d) in &events {
            s.replay(k, d).expect("replay");
        }
        let e = &s.runs["r1"];
        let t = &e.tasks["t1"];
        assert_eq!(t.routing, 0);
        assert!(!t.pinned);
        assert_eq!((t.quality_floor, t.quality_attempts, t.operational_attempts), (0, 0, 0));
        assert!(t.wait.is_none() && t.last_failure.is_none());
        assert_eq!(e.run.autonomy, Autonomy::default());
        assert_eq!(e.run.limits.max_operational_attempts, 6);
        let d = &e.dispatches["r1.t1.1"];
        assert!(d.chain_index.is_none() && d.trigger.is_none() && d.decision.is_none());
        assert!(d.failure_class.is_none() && d.by.is_none() && d.account.is_none());
    }

    #[test]
    fn f12_legacy_limit_check_unchanged_for_routing_zero() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        s.runs.get_mut("r1").unwrap().run.limits.max_attempts = 2;
        mk_task(&mut s, "r1", "t1", vec![]);
        let d1 = mk_dispatch(&mut s, "r1", "t1", 1);
        fail_dispatch(&mut s, "r1", &d1);
        let d2 = mk_dispatch(&mut s, "r1", "t1", 2);
        fail_dispatch(&mut s, "r1", &d2);
        let err = commit_cmd(&mut s, dispatch_cmd("r1", "t1", 3)).unwrap_err();
        assert_eq!(err.code(), "limit");
        assert_eq!(err.message(), "max attempts reached");
    }

    #[test]
    fn f12_routed_task_create_stores_routing_and_pinned() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        let cmd = Cmd::TaskCreate {
            run_id: "r1".into(), id: "t1".into(), title: "t".into(), spec: "s".into(),
            acceptance: None, deps: vec![], profile: Some("claude".into()), escalation: vec![],
            verify: vec![], role: None, review: false, owner: None, now: ts(),
        }.with_fields(serde_json::json!({"routing": 1, "pinned": true}));
        commit_cmd(&mut s, cmd).unwrap();
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!(t.routing, 1);
        assert!(t.pinned);
        let json = serde_json::to_value(t).unwrap();
        assert_eq!(json["routing"], 1);
        assert_eq!(json["qualityFloor"], 0);
        assert_eq!(json["pinned"], true);
    }

    #[test]
    fn f12_task_update_profile_marks_pinned() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_routed_task(&mut s, "r1", "t1");
        let cmd = Cmd::TaskUpdate { owner: None, by: "test".into(), reason: None,
            run_id: "r1".into(), task_id: "t1".into(), status: None, result: None, spec: None,
            acceptance: None, verify: None, profile: Some("codex".into()), escalation: None,
            review: None, now: ts(),
        };
        commit_cmd(&mut s, cmd).unwrap();
        assert!(s.runs["r1"].tasks["t1"].pinned);
    }

    #[test]
    fn f12_routed_limit_uses_quality_attempts_not_dispatch_count() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        s.runs.get_mut("r1").unwrap().run.limits.max_attempts = 2;
        mk_routed_task(&mut s, "r1", "t1");
        for attempt in 1..=4u32 {
            let d = mk_dispatch(&mut s, "r1", "t1", attempt);
            fail_dispatch(&mut s, "r1", &d);
            record_failure(&mut s, &d, FailureClass::RateLimited, Some(0));
        }
        assert_eq!(s.runs["r1"].tasks["t1"].attempts, 4);
        let d5 = mk_dispatch(&mut s, "r1", "t1", 5);
        fail_dispatch(&mut s, "r1", &d5);
        record_failure(&mut s, &d5, FailureClass::QualityFailed, Some(0));
        let d6 = mk_dispatch(&mut s, "r1", "t1", 6);
        fail_dispatch(&mut s, "r1", &d6);
        record_failure(&mut s, &d6, FailureClass::QualityFailed, Some(1));
        let err = commit_cmd(&mut s, dispatch_cmd("r1", "t1", 7)).unwrap_err();
        assert_eq!(err.code(), "limit");
        assert_eq!(err.message(), "max attempts reached");
    }

    #[test]
    fn f12_routed_limit_uses_operational_attempts() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        s.runs.get_mut("r1").unwrap().run.limits.max_operational_attempts = 2;
        mk_routed_task(&mut s, "r1", "t1");
        for attempt in 1..=2u32 {
            let d = mk_dispatch(&mut s, "r1", "t1", attempt);
            fail_dispatch(&mut s, "r1", &d);
            record_failure(&mut s, &d, FailureClass::LaunchFailed, None);
        }
        let err = commit_cmd(&mut s, dispatch_cmd("r1", "t1", 3)).unwrap_err();
        assert_eq!(err.code(), "limit");
        assert_eq!(err.message(), "max operational attempts reached");
    }

    #[test]
    fn f12_dispatch_event_stores_decision_fields_and_replays() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_routed_task(&mut s, "r1", "t1");
        let decision = serde_json::json!({
            "requested": {"agent": "claude", "model": "opus"},
            "effective": {"agent": "codex"},
            "reason": "fallback: claude:opus rate_limited",
            "skipped": [{"candidate": {"agent": "claude", "model": "opus"}, "state": "rate_limited", "reason": "claude:opus rate_limited"}]
        });
        let cmd = dispatch_cmd("r1", "t1", 1).with_fields(serde_json::json!({
            "chainIndex": 1, "trigger": "fallback", "decision": decision, "account": "acc1", "by": "brain"
        }));
        let (_, data) = commit_cmd(&mut s, cmd).unwrap();
        assert_eq!(data["chainIndex"], 1);
        let d = &s.runs["r1"].dispatches["r1.t1.1"];
        assert_eq!(d.chain_index, Some(1));
        assert_eq!(d.trigger, Some(Trigger::Fallback));
        assert_eq!(d.by, Some(DispatchBy::Brain));
        assert_eq!(d.account.as_deref(), Some("acc1"));
        let dec = d.decision.as_ref().unwrap();
        assert_eq!(dec.effective.agent, "codex");
        assert_eq!(dec.skipped.len(), 1);
        assert_eq!(dec.skipped[0].state, crate::orch::candidates::CandidateStatus::RateLimited);
    }

    #[test]
    fn f12_failure_class_rides_on_dispatch_update_worker_done_and_verification_failed() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_routed_task(&mut s, "r1", "t1");
        let d1 = mk_dispatch(&mut s, "r1", "t1", 1);
        let cmd = Cmd::DispatchUpdate {
            run_id: "r1".into(), dispatch_id: d1.clone(), status: DispatchStatus::Failed, summary: None, now: ts(),
        }.with_fields(serde_json::json!({"failureClass": "launch_failed"}));
        commit_cmd(&mut s, cmd).unwrap();
        assert_eq!(s.runs["r1"].dispatches[&d1].failure_class, Some(FailureClass::LaunchFailed));

        let d2 = mk_dispatch(&mut s, "r1", "t1", 2);
        let cmd = Cmd::Send {
            run_id: "r1".into(), seq: 1, from: d2.clone(), to: vec!["coordinator".into()],
            msg_type: "worker_done".into(), task_id: None, dispatch_id: None, reply_to: None, body: None,
            outcome: Some("failed".into()), files_modified: vec![], summary: None, now: ts(),
        }.with_fields(serde_json::json!({"failureClass": "quality_failed"}));
        commit_cmd(&mut s, cmd).unwrap();
        assert_eq!(s.runs["r1"].dispatches[&d2].failure_class, Some(FailureClass::QualityFailed));

        let mut s2 = State::default();
        mk_run(&mut s2, "r1");
        mk_task_with_verify(&mut s2, "r1", "t1", false);
        let d = mk_dispatch(&mut s2, "r1", "t1", 1);
        set_task_verifying(&mut s2, "r1", "t1");
        let cmd = Cmd::TaskVerificationFailed {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: d.clone(), command: "c".into(),
            exit_code: Some(1), timed_out: false, tail: "x".into(), msg_seq: 1, now: ts(),
        }.with_fields(serde_json::json!({"failureClass": "quality_failed"}));
        commit_cmd(&mut s2, cmd).unwrap();
        assert_eq!(s2.runs["r1"].dispatches[&d].failure_class, Some(FailureClass::QualityFailed));
    }

    #[test]
    fn f12_extra_fields_do_not_mask_rejections() {
        let mut s = State::default();
        let cmd = dispatch_cmd("nope", "t1", 1).with_fields(serde_json::json!({"trigger": "initial"}));
        assert_eq!(commit_cmd(&mut s, cmd).unwrap_err().code(), "not_found");
    }

    #[test]
    fn f12_task_wait_sets_waiting_with_deterministic_resume_id() {
        let mut s = routed_failed_task();
        let (_, data) = commit_cmd(&mut s, wait_cmd(Some(5000))).unwrap();
        assert_eq!(data["resumeId"], "t1:1:5000");
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!(t.status, TaskStatus::Waiting);
        let w = t.wait.as_ref().unwrap();
        assert_eq!(w.kind, WaitKind::Quota);
        assert_eq!(w.resume_at, Some(5000));
        assert_eq!(w.resume_id, "t1:1:5000");
        assert_eq!(w.candidate.as_ref().unwrap().model.as_deref(), Some("opus"));
        assert!(!w.same_session);
    }

    #[test]
    fn f12_task_wait_without_resume_time_uses_none_in_id() {
        let mut s = routed_failed_task();
        commit_cmd(&mut s, wait_cmd(None)).unwrap();
        assert_eq!(s.runs["r1"].tasks["t1"].wait.as_ref().unwrap().resume_id, "t1:1:none");
    }

    #[test]
    fn f12_task_wait_rejected_for_running_or_terminal_tasks() {
        let mut s = State::default();
        mk_run(&mut s, "r1");
        mk_routed_task(&mut s, "r1", "t1");
        mk_dispatch(&mut s, "r1", "t1", 1);
        assert_eq!(commit_cmd(&mut s, wait_cmd(None)).unwrap_err().code(), "conflict");
    }

    #[test]
    fn f12_task_resume_is_idempotent_per_resume_id() {
        let mut s = routed_failed_task();
        commit_cmd(&mut s, wait_cmd(Some(5000))).unwrap();
        commit_cmd(&mut s, resume_cmd("t1:1:5000")).unwrap();
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!(t.status, TaskStatus::Pending);
        assert!(t.wait.is_none());
        let again = commit_cmd(&mut s, resume_cmd("t1:1:5000")).unwrap_err();
        assert_eq!(again.code(), "conflict");
    }

    #[test]
    fn f12_task_resume_rejects_stale_resume_id() {
        let mut s = routed_failed_task();
        commit_cmd(&mut s, wait_cmd(Some(5000))).unwrap();
        assert_eq!(commit_cmd(&mut s, resume_cmd("t1:1:4000")).unwrap_err().code(), "conflict");
        assert_eq!(s.runs["r1"].tasks["t1"].status, TaskStatus::Waiting);
        commit_cmd(&mut s, resume_cmd("t1:1:5000")).unwrap();
    }

    #[test]
    fn f12_resumed_task_can_dispatch_and_dispatch_clears_wait() {
        let mut s = routed_failed_task();
        commit_cmd(&mut s, wait_cmd(Some(5000))).unwrap();
        commit_cmd(&mut s, resume_cmd("t1:1:5000")).unwrap();
        commit_cmd(&mut s, dispatch_cmd("r1", "t1", 2)).unwrap();
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!(t.status, TaskStatus::Dispatched);
        assert_eq!(t.attempts, 2);
    }

    #[test]
    fn f12_waiting_task_can_be_cancelled_and_clears_wait() {
        let mut s = routed_failed_task();
        commit_cmd(&mut s, wait_cmd(None)).unwrap();
        let cmd = Cmd::TaskUpdate { owner: None, by: "test".into(), reason: None,
            run_id: "r1".into(), task_id: "t1".into(), status: Some(TaskStatus::Cancelled), result: None,
            spec: None, acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
        };
        commit_cmd(&mut s, cmd).unwrap();
        let t = &s.runs["r1"].tasks["t1"];
        assert_eq!(t.status, TaskStatus::Cancelled);
        assert!(t.wait.is_none());
    }

    #[test]
    fn f12_wait_and_resume_replay_from_event_log() {
        let mut live = routed_failed_task();
        let mut log: Vec<(String, Value)> = vec![];
        for cmd in [wait_cmd(Some(5000)), resume_cmd("t1:1:5000"), wait_cmd(Some(7000))] {
            if let Ok(ev) = commit_cmd(&mut live, cmd) {
                log.push(ev);
            }
        }
        let mut fresh = State::default();
        mk_run(&mut fresh, "r1");
        mk_routed_task(&mut fresh, "r1", "t1");
        let d = mk_dispatch(&mut fresh, "r1", "t1", 1);
        fail_dispatch(&mut fresh, "r1", &d);
        for (k, d) in &log {
            fresh.replay(k, d).unwrap();
        }
        assert_eq!(fresh.runs["r1"].tasks["t1"].status, live.runs["r1"].tasks["t1"].status);
        assert_eq!(fresh.runs["r1"].tasks["t1"].wait, live.runs["r1"].tasks["t1"].wait);
        assert_eq!(live.runs["r1"].tasks["t1"].status, TaskStatus::Waiting);
    }

    fn counters(s: &State) -> (u32, u32, u32, Option<FailureClass>) {
        let t = &s.runs["r1"].tasks["t1"];
        (t.quality_attempts, t.operational_attempts, t.quality_floor, t.last_failure)
    }

    #[test]
    fn f12_record_failure_counter_table_for_every_class() {
        let operational = [
            FailureClass::QuotaExhausted, FailureClass::RateLimited, FailureClass::AuthFailed,
            FailureClass::ModelUnavailable, FailureClass::LaunchFailed, FailureClass::ExecutionFailed,
        ];
        for class in operational {
            let mut s = routed_failed_task();
            record_failure(&mut s, "r1.t1.1", class, Some(0));
            assert_eq!(counters(&s), (0, 1, 0, Some(class)), "{class:?}");
        }
        let mut s = routed_failed_task();
        record_failure(&mut s, "r1.t1.1", FailureClass::SetupFailed, Some(0));
        assert_eq!(counters(&s), (0, 0, 0, Some(FailureClass::SetupFailed)));
        for class in [FailureClass::QualityFailed, FailureClass::Unknown] {
            let mut s = routed_failed_task();
            record_failure(&mut s, "r1.t1.1", class, Some(0));
            assert_eq!(counters(&s), (1, 0, 1, Some(class)), "{class:?}");
        }
    }

    #[test]
    fn f12_record_failure_sets_class_on_dispatch() {
        let mut s = routed_failed_task();
        record_failure(&mut s, "r1.t1.1", FailureClass::AuthFailed, None);
        assert_eq!(s.runs["r1"].dispatches["r1.t1.1"].failure_class, Some(FailureClass::AuthFailed));
    }

    #[test]
    fn f12_quality_floor_never_decreases() {
        let mut s = routed_failed_task();
        record_failure(&mut s, "r1.t1.1", FailureClass::QualityFailed, Some(2));
        assert_eq!(counters(&s).2, 3);
        record_on_new_dispatch(&mut s, 2, FailureClass::QualityFailed, Some(0));
        assert_eq!(counters(&s).2, 3);
        record_on_new_dispatch(&mut s, 3, FailureClass::QualityFailed, None);
        assert_eq!(counters(&s), (3, 0, 3, Some(FailureClass::QualityFailed)));
    }

    #[test]
    fn f12_operational_failure_leaves_floor_and_quality_attempts_unchanged() {
        let mut s = routed_failed_task();
        record_failure(&mut s, "r1.t1.1", FailureClass::QualityFailed, Some(0));
        let before = counters(&s);
        for (attempt, class) in [(2, FailureClass::QuotaExhausted), (3, FailureClass::LaunchFailed), (4, FailureClass::ExecutionFailed)] {
            record_on_new_dispatch(&mut s, attempt, class, Some(5));
        }
        let after = counters(&s);
        assert_eq!((after.0, after.2), (before.0, before.2));
        assert_eq!(after.1, 3);
        assert_eq!(after.3, Some(FailureClass::ExecutionFailed));
    }

    #[test]
    fn f12_quality_failure_survives_later_fallback_failures() {
        let mut s = routed_failed_task();
        record_failure(&mut s, "r1.t1.1", FailureClass::QualityFailed, Some(0));
        record_on_new_dispatch(&mut s, 2, FailureClass::RateLimited, Some(1));
        record_on_new_dispatch(&mut s, 3, FailureClass::QualityFailed, Some(2));
        let (q, o, floor, _) = counters(&s);
        assert_eq!((q, o, floor), (2, 1, 3));
    }

    #[test]
    fn f12_record_failure_validates_task_and_dispatch() {
        let mut s = routed_failed_task();
        let bad_dispatch = Cmd::TaskRecordFailure {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t9.1".into(),
            class: FailureClass::Unknown, chain_index: None, now: ts(),
        };
        assert_eq!(commit_cmd(&mut s, bad_dispatch).unwrap_err().code(), "not_found");
        let bad_task = Cmd::TaskRecordFailure {
            run_id: "r1".into(), task_id: "t9".into(), dispatch_id: "r1.t1.1".into(),
            class: FailureClass::Unknown, chain_index: None, now: ts(),
        };
        assert_eq!(commit_cmd(&mut s, bad_task).unwrap_err().code(), "not_found");
    }

    #[test]
    fn f12_second_record_failure_for_same_dispatch_is_rejected_and_counts_once() {
        let mut s = routed_failed_task();
        record_failure(&mut s, "r1.t1.1", FailureClass::RateLimited, Some(0));
        let again = Cmd::TaskRecordFailure {
            run_id: "r1".into(), task_id: "t1".into(), dispatch_id: "r1.t1.1".into(),
            class: FailureClass::ExecutionFailed, chain_index: Some(0), now: ts(),
        };
        assert_eq!(commit_cmd(&mut s, again).unwrap_err().code(), "conflict");
        assert_eq!(counters(&s), (0, 1, 0, Some(FailureClass::RateLimited)));
    }

    #[test]
    fn f12_record_failure_replays_to_same_counters() {
        let mut live = routed_failed_task();
        let mut log = vec![];
        for (attempt, (class, idx)) in [(FailureClass::QualityFailed, Some(0)), (FailureClass::RateLimited, Some(1)), (FailureClass::Unknown, Some(2))].into_iter().enumerate() {
            let id = if attempt == 0 {
                "r1.t1.1".to_string()
            } else {
                let id = mk_dispatch(&mut live, "r1", "t1", attempt as u32 + 1);
                fail_dispatch(&mut live, "r1", &id);
                id
            };
            let cmd = Cmd::TaskRecordFailure {
                run_id: "r1".into(), task_id: "t1".into(), dispatch_id: id,
                class, chain_index: idx, now: ts(),
            };
            log.push(commit_cmd(&mut live, cmd).unwrap());
        }
        let mut fresh = State::default();
        mk_run(&mut fresh, "r1");
        mk_routed_task(&mut fresh, "r1", "t1");
        for attempt in 1..=3 {
            let d = mk_dispatch(&mut fresh, "r1", "t1", attempt);
            fail_dispatch(&mut fresh, "r1", &d);
        }
        for (k, d) in &log {
            fresh.replay(k, d).unwrap();
        }
        assert_eq!(counters(&fresh), counters(&live));
        assert_eq!(counters(&live), (2, 1, 3, Some(FailureClass::Unknown)));
    }

    #[test]
    fn f12_run_create_and_update_store_autonomy_and_max_operational() {
        let mut s = State::default();
        let create = Cmd::RunCreate {
            id: "r1".into(), title: "t".into(), repo: "/x".into(), base_ref: None, base_sha: None,
            setup: None, coordinator_tile: None, limits: Limits::default(), now: ts(),
        }.with_fields(serde_json::json!({"autonomy": {"operational": true, "execution": false, "integration": false}}));
        commit_cmd(&mut s, create).unwrap();
        assert!(s.runs["r1"].run.autonomy.operational);
        assert!(!s.runs["r1"].run.autonomy.execution);

        let update = Cmd::RunUpdate { run_id: "r1".into(), title: "t".into(), now: ts() }
            .with_fields(serde_json::json!({
                "autonomy": {"operational": false, "execution": true, "integration": true},
                "maxOperationalAttempts": 9
            }));
        commit_cmd(&mut s, update).unwrap();
        let run = &s.runs["r1"].run;
        assert_eq!(run.autonomy, Autonomy { operational: false, execution: true, integration: true });
        assert_eq!(run.limits.max_operational_attempts, 9);

        let clear = Cmd::RunUpdate { run_id: "r1".into(), title: "t".into(), now: ts() }
            .with_fields(serde_json::json!({"autonomy": {}}));
        commit_cmd(&mut s, clear).unwrap();
        assert_eq!(s.runs["r1"].run.autonomy, Autonomy::default());
        assert_eq!(s.runs["r1"].run.limits.max_operational_attempts, 9);
    }

    #[test]
    fn f12_two_dispatches_racing_for_last_slot_record_exactly_one() {
        use std::sync::{Arc, Mutex};

        for _ in 0..20 {
            let state = Arc::new(Mutex::new(State::default()));
            {
                let mut g = state.lock().unwrap();
                mk_run(&mut g, "r1");
                g.runs.get_mut("r1").unwrap().run.limits.max_workers = 1;
                mk_routed_task(&mut g, "r1", "t1");
                mk_routed_task(&mut g, "r1", "t2");
            }
            let handles: Vec<_> = ["t1", "t2"].into_iter().map(|task_id| {
                let state = state.clone();
                std::thread::spawn(move || {
                    let mut g = state.lock().unwrap();
                    commit_cmd(&mut g, dispatch_cmd("r1", task_id, 1)).is_ok()
                })
            }).collect();
            let accepted = handles.into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count();
            assert_eq!(accepted, 1);
            assert_eq!(state.lock().unwrap().runs["r1"].running_dispatch_count(), 1);
        }
    }

    #[test]
    fn f12_failure_class_serde_values_are_snake_case() {
        let v = serde_json::to_value([FailureClass::QuotaExhausted, FailureClass::SetupFailed]).unwrap();
        assert_eq!(v, serde_json::json!(["quota_exhausted", "setup_failed"]));
        assert_eq!(serde_json::to_value(TaskStatus::Waiting).unwrap(), "waiting");
        assert_eq!(serde_json::to_value(Trigger::Escalation).unwrap(), "escalation");
        assert_eq!(serde_json::to_value(WaitKind::Intervention).unwrap(), "intervention");
    }
}
