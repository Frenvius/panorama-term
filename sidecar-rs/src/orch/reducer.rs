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

pub struct RunEntry {
    pub run: Run,
    pub tasks: HashMap<String, Task>,
    pub dispatches: HashMap<String, Dispatch>,
    pub messages: Vec<Message>,
    pub gates: HashMap<String, Gate>,
    pub task_count: u32,
    pub gate_count: u32,
    pub msg_seq: u32,
    pub event_seq: u32,
}

impl RunEntry {
    fn new(run: Run) -> Self {
        RunEntry {
            run,
            tasks: HashMap::new(),
            dispatches: HashMap::new(),
            messages: Vec::new(),
            gates: HashMap::new(),
            task_count: 0,
            gate_count: 0,
            msg_seq: 0,
            event_seq: 0,
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
        review: bool,
        owner: Option<String>,
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
        integrated_sha: String,
        msg_seq: u32,
        now: u64,
    },
    TaskConflict {
        run_id: String,
        task_id: String,
        files: Vec<String>,
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
}

impl State {
    pub fn apply(&self, cmd: &Cmd) -> ApplyResult {
        match cmd {
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

            Cmd::TaskCreate { run_id, id, title, spec, acceptance, deps, profile, escalation, verify, review, owner, now } => {
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
                    "escalation": escalation, "verify": verify, "review": review,
                    "owner": owner, "now": now
                }))
            }

            Cmd::TaskUpdate { run_id, task_id, status, result, spec, acceptance, verify, profile, escalation, review, now } => {
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
                    "profile": profile, "escalation": escalation, "review": review, "now": now
                }))
            }

            Cmd::Dispatch { run_id, id, task_id, attempt, retry_of, profile, model, tile_id, owner, now } => {
                let Some(entry) = self.runs.get(run_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("run not found".into()));
                };
                if entry.run.status != RunStatus::Active {
                    return ApplyResult::Err(OrchError::Conflict("run not active".into()));
                }
                let Some(task) = entry.tasks.get(task_id.as_str()) else {
                    return ApplyResult::Err(OrchError::NotFound("task not found".into()));
                };
                let can_dispatch = entry.task_ready(task) || task.status == TaskStatus::Failed;
                if !can_dispatch {
                    return ApplyResult::Err(OrchError::Conflict("task not ready for dispatch".into()));
                }
                if task.attempts >= entry.run.limits.max_attempts {
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
                    "tileId": tile_id, "owner": owner, "now": now
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
                    }
                    if dispatch.status != DispatchStatus::Running {
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

            Cmd::TaskIntegrate { run_id, task_id, integrated_sha, msg_seq, now } => {
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
                let latest = entry.latest_dispatch_for_task(task_id);
                if latest.and_then(|d| d.branch.as_ref()).is_none() {
                    return ApplyResult::Err(OrchError::BadRequest(
                        "latest dispatch has no branch".into()
                    ));
                }
                ApplyResult::Ok("task-integrate".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id,
                    "integratedSha": integrated_sha, "msgSeq": msg_seq, "now": now
                }))
            }

            Cmd::TaskConflict { run_id, task_id, files, msg_seq, now } => {
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
                ApplyResult::Ok("task-conflict".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id,
                    "files": files, "msgSeq": msg_seq, "now": now
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
                ApplyResult::Ok("task-verification-failed".into(), serde_json::json!({
                    "runId": run_id, "taskId": task_id, "dispatchId": dispatch_id,
                    "command": command, "exitCode": exit_code, "timedOut": timed_out,
                    "tail": tail, "msgSeq": msg_seq, "now": now
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
        }
    }

    pub fn replay(&mut self, kind: &str, data: &Value) -> Result<(), String> {
        match kind {
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
            "dispatch-record-head" => self.replay_dispatch_record_head(data),
            "task-integrate" => self.replay_task_integrate(data),
            "task-conflict" => self.replay_task_conflict(data),
            "dispatch-append-verification" => self.replay_dispatch_append_verification(data),
            "dispatch-record-cost" => self.replay_dispatch_record_cost(data),
            "task-verified" => self.replay_task_verified(data),
            "task-verification-failed" => self.replay_task_verification_failed(data),
            "task-re-verify" => self.replay_task_re_verify(data),
            other => Err(format!("unknown event kind: {other}")),
        }
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
        let task = Task {
            id: id.clone(),
            title: str_field(data, "title"),
            spec: str_field(data, "spec"),
            acceptance: opt_str(data, "acceptance"),
            deps: str_arr(data, "deps"),
            profile: opt_str(data, "profile"),
            escalation: str_arr(data, "escalation"),
            verify: str_arr(data, "verify"),
            review: data["review"].as_bool().unwrap_or(false),
            owner: opt_str(data, "owner"),
            status: TaskStatus::Pending,
            result: None,
            integrated_sha: None,
            attempts: 0,
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

        let Some(task) = entry.tasks.get_mut(&task_id) else { return Ok(()) };
        if let Some(s) = new_status {
            task.status = s;
        }
        if let Some(v) = opt_val_str(data, "result") { task.result = Some(v); }
        if let Some(v) = opt_val_str(data, "spec") { task.spec = v; }
        if let Some(v) = opt_val_str(data, "acceptance") { task.acceptance = Some(v); }
        if let Some(arr) = opt_val_arr(data, "verify") { task.verify = arr; }
        if let Some(v) = opt_val_str(data, "profile") { task.profile = Some(v); }
        if let Some(arr) = opt_val_arr(data, "escalation") { task.escalation = arr; }
        if let Some(v) = data.get("review").filter(|v| !v.is_null()).and_then(|v| v.as_bool()) {
            task.review = v;
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
            worktree: None,
            branch: None,
            status: DispatchStatus::Running,
            outcome: None,
            head_sha: None,
            files_modified: vec![],
            summary: None,
            started_at: now,
            ended_at: None,
            last_heartbeat: None,
            verifications: vec![],
            cost_usd: None,
            cost_source: None,
        };
        if let Some(owner) = opt_str(data, "owner") {
            if let Some(task) = entry.tasks.get_mut(&task_id) {
                task.owner = Some(owner);
            }
        }
        entry.dispatches.insert(dispatch_id, dispatch);
        if let Some(task) = entry.tasks.get_mut(&task_id) {
            task.status = TaskStatus::Dispatched;
            task.attempts = attempt;
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
            d.worktree = opt_str(data, "worktree");
            d.branch = opt_str(data, "branch");
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
        let msg = Message {
            seq: msg_seq,
            from: "brain".to_string(),
            to: "coordinator".to_string(),
            msg_type: "task_integrated".to_string(),
            task_id: Some(task_id),
            dispatch_id: None,
            reply_to: None,
            body: opt_str(data, "integratedSha"),
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
            body: Some(serde_json::to_string(&data["files"]).unwrap_or_default()),
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
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            attempt: 4, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            attempt: 2, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            attempt: 2, retry_of: Some("r1.t1.1".into()), profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            attempt: 2, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            verify: vec!["bun test".into()], review: false, owner: None, now: ts(),
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
                acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            worktree: None, branch: None,
            status: DispatchStatus::Running, outcome: None, head_sha: None, files_modified: vec![],
            summary: None, started_at: ts(), ended_at: None, last_heartbeat: None,
            verifications: vec![], cost_usd: None, cost_source: None,
        };
        s.runs.get_mut("r1").unwrap().dispatches.insert("r1.t1.2".into(), d2);

        let update = Cmd::TaskUpdate {
            run_id: "r1".into(), task_id: "t1".into(),
            status: Some(TaskStatus::Failed), result: None, spec: None,
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            profile: None, escalation: vec![], verify: vec![], review: false, owner: None, now: ts(),
        });
        apply_capture!(Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
                        attempt, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            run_id: "r1".into(), task_id: "t1".into(),
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
            run_id: "r1".into(), task_id: "t1".into(),
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
            files: vec!["a.rs".into(), "b.rs".into()], msg_seq: 2, now: ts(),
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
            verify: vec!["check".into()], review: false, owner: None, now: ts(),
        });
        apply_ev!(Cmd::Dispatch {
            run_id: "r1".into(), id: "r1.t1.1".into(), task_id: "t1".into(),
            attempt: 1, retry_of: None, profile: None, model: None, tile_id: None, owner: None, now: ts(),
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            acceptance: None, verify: None, profile: None, escalation: None, review: None, now: ts(),
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
            owner: Some("user".into()), now: ts(),
        };
        match s.apply(&cmd) {
            ApplyResult::Ok(k, d) => s.replay(&k, &d).expect("replay"),
            _ => panic!("dispatch failed"),
        }
        assert_eq!(s.runs["r1"].tasks["t1"].owner.as_deref(), Some("user"));
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
}
