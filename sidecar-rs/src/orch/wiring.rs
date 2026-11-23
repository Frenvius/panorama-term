use serde_json::{json, Value};
use crate::orch::candidates::{CandidateKey, CandidateStates, Confidence as CandidateConfidence, MarkInput};
use crate::orch::failure::{self, Classification, ClassifyInput, Confidence as ClassConfidence, Scope};
use crate::orch::model::*;
use crate::orch::reducer::{Cmd, RunEntry, WaitRequest};
use crate::orch::routing::{self, RouteInput, RouteOutcome};

pub(crate) struct PlanInput<'a> {
    pub entry: &'a RunEntry,
    pub task_id: &'a str,
    pub explicit_profile: Option<&'a str>,
    pub explicit_model: Option<&'a str>,
    pub by: DispatchBy,
    pub resumed: bool,
    pub now: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct PlannedDispatch {
    pub profile: String,
    pub model: Option<String>,
    pub chain_index: Option<u32>,
    pub trigger: Trigger,
    pub decision: RoutingDecision,
    pub by: DispatchBy,
}

pub(crate) enum DispatchPlan {
    Dispatch(PlannedDispatch),
    Wait(WaitRequest),
}

pub(crate) enum RetryLimit {
    Open,
    QualityExhausted,
    OperationalExhausted,
}

pub(crate) struct AfterFailure {
    pub wait: Option<WaitRequest>,
    pub suggested: Option<CandidateKey>,
}

fn chain_of(task: &Task) -> Vec<CandidateKey> {
    if !task.escalation.is_empty() {
        return task.escalation.iter().map(|e| CandidateKey::from_chain_entry(e)).collect();
    }
    task.profile.iter().map(|p| CandidateKey::from_chain_entry(p)).collect()
}

fn pinned_key(task: &Task, chain: &[CandidateKey], input: &PlanInput) -> Option<CandidateKey> {
    let window_head = || {
        let start = (task.quality_floor as usize).min(chain.len().saturating_sub(1));
        chain.get(start).cloned()
    };
    match (input.explicit_profile, input.explicit_model) {
        (Some(profile), model) => Some(CandidateKey {
            agent: profile.to_string(),
            model: model.map(str::to_string),
            account: None,
        }),
        (None, Some(model)) => window_head().map(|head| CandidateKey { model: Some(model.to_string()), ..head }),
        (None, None) if task.pinned => task.profile.as_deref().map(CandidateKey::from_chain_entry),
        (None, None) => None,
    }
}

fn trigger_for(task: &Task, explicit: bool, resumed: bool) -> Trigger {
    if explicit {
        return Trigger::Manual;
    }
    if resumed {
        return Trigger::Resume;
    }
    if task.attempts == 0 {
        return Trigger::Initial;
    }
    match task.last_failure.map(FailureClass::counter) {
        Some(FailureCounter::Operational) => Trigger::Fallback,
        Some(FailureCounter::Quality) => Trigger::Escalation,
        _ => Trigger::Resume,
    }
}

fn wait_request(kind: WaitKind, resume_at: Option<i64>, reason: String, candidate: Option<CandidateKey>) -> WaitRequest {
    WaitRequest { kind, resume_at, reason, candidate, same_session: false }
}

pub(crate) fn plan_dispatch(input: PlanInput, states: &CandidateStates) -> Result<DispatchPlan, String> {
    let task = input.entry.tasks.get(input.task_id)
        .ok_or_else(|| "not_found:task not found".to_string())?;
    let chain = chain_of(task);
    let pinned = pinned_key(task, &chain, &input);
    if pinned.is_none() && chain.is_empty() {
        return Err("bad_request:profile required (no escalation list and no task profile)".into());
    }
    let explicit = input.explicit_profile.is_some() || input.explicit_model.is_some();
    let requested = pinned.clone().or_else(|| {
        let start = (task.quality_floor as usize).min(chain.len().saturating_sub(1));
        chain.get(start).cloned()
    });
    let outcome = routing::route(RouteInput {
        chain: &chain,
        floor: task.quality_floor,
        pinned: pinned.as_ref(),
        states,
        now: input.now,
        short_wait_ms: routing::DEFAULT_SHORT_WAIT_MS,
    });
    Ok(match outcome {
        RouteOutcome::Dispatch { candidate, chain_index, reason, skipped } => DispatchPlan::Dispatch(PlannedDispatch {
            profile: candidate.agent.clone(),
            model: candidate.model.clone(),
            chain_index,
            trigger: trigger_for(task, explicit, input.resumed),
            decision: RoutingDecision { requested, effective: candidate, reason, skipped },
            by: input.by,
        }),
        RouteOutcome::Wait { kind, resume_at, candidate, reason, .. } => {
            DispatchPlan::Wait(wait_request(kind, resume_at, reason, candidate))
        }
    })
}

pub(crate) fn dispatch_fields(planned: &PlannedDispatch, account: Option<String>) -> Value {
    json!({
        "chainIndex": planned.chain_index,
        "trigger": planned.trigger,
        "decision": planned.decision,
        "account": account,
        "by": planned.by,
    })
}

pub(crate) fn dispatch_by(raw: &str) -> DispatchBy {
    match raw {
        "user" => DispatchBy::User,
        "brain" => DispatchBy::Brain,
        _ => DispatchBy::Coordinator,
    }
}

pub(crate) fn retry_limit(entry: &RunEntry, task: &Task) -> RetryLimit {
    if task.quality_attempts >= entry.run.limits.max_attempts {
        return RetryLimit::QualityExhausted;
    }
    if task.operational_attempts >= entry.run.limits.max_operational_attempts {
        return RetryLimit::OperationalExhausted;
    }
    RetryLimit::Open
}

pub(crate) fn operational_cap_wait() -> WaitRequest {
    wait_request(WaitKind::Intervention, None, "operational attempts exhausted".into(), None)
}

pub(crate) fn dispatch_key(dispatch: &Dispatch) -> Option<CandidateKey> {
    Some(CandidateKey {
        agent: dispatch.profile.clone()?,
        model: dispatch.model.clone(),
        account: dispatch.account.clone(),
    })
}

pub(crate) fn classify_failure(agent: Option<&str>, reason_code: &str, tail: &[String], now: i64) -> Classification {
    failure::classify(ClassifyInput {
        agent: agent.unwrap_or(""),
        reason_code,
        tail,
        now_ms: now,
        local_offset_min: local_offset_min(),
    })
}

pub(crate) fn is_live_quota_class(class: FailureClass) -> bool {
    matches!(class, FailureClass::QuotaExhausted | FailureClass::RateLimited | FailureClass::AuthFailed)
}

pub(crate) fn mark_candidate(states: &mut CandidateStates, dispatch: &Dispatch, found: &Classification, now: i64) -> bool {
    let Some(mut key) = dispatch_key(dispatch) else { return false };
    if found.scope == Scope::Account {
        key.model = None;
    }
    let confidence = match found.confidence {
        ClassConfidence::Reliable => CandidateConfidence::Reliable,
        ClassConfidence::Partial => CandidateConfidence::Partial,
    };
    states.mark(MarkInput {
        key: &key,
        class: found.class,
        reset_at: found.reset_at,
        source: &found.source,
        confidence,
        now,
    })
}

pub(crate) struct AfterFailureInput<'a> {
    pub entry: &'a RunEntry,
    pub states: &'a CandidateStates,
    pub dispatch_id: &'a str,
    pub live: bool,
    pub now: i64,
}

pub(crate) fn after_failure(input: AfterFailureInput) -> AfterFailure {
    let AfterFailureInput { entry, states, dispatch_id, live, now } = input;
    let none = AfterFailure { wait: None, suggested: None };
    let Some(dispatch) = entry.dispatches.get(dispatch_id) else { return none };
    let Some(task) = entry.tasks.get(&dispatch.task_id) else { return none };
    let Some(class) = dispatch.failure_class.filter(|_| task.routing == 1) else { return none };
    if class == FailureClass::SetupFailed {
        let wait = wait_request(WaitKind::Intervention, None, "setup failed; fix the run setup command".into(), None);
        return AfterFailure { wait: Some(wait), suggested: None };
    }
    let counter = class.counter();
    if counter == FailureCounter::Quality && task.quality_attempts >= entry.run.limits.max_attempts {
        return none;
    }
    if counter == FailureCounter::Operational && task.operational_attempts >= entry.run.limits.max_operational_attempts {
        return AfterFailure { wait: Some(operational_cap_wait()), suggested: None };
    }
    let planned = plan_dispatch(PlanInput {
        entry,
        task_id: &task.id,
        explicit_profile: None,
        explicit_model: None,
        by: DispatchBy::Brain,
        resumed: false,
        now,
    }, states);
    match planned {
        Ok(DispatchPlan::Wait(mut wait)) => {
            let same = wait.candidate.as_ref().zip(dispatch_key(dispatch))
                .is_some_and(|(w, d)| w.agent == d.agent && (w.model.is_none() || w.model == d.model));
            wait.same_session = same && live && is_live_quota_class(class);
            AfterFailure { wait: Some(wait), suggested: None }
        }
        Ok(DispatchPlan::Dispatch(p)) => AfterFailure { wait: None, suggested: Some(p.decision.effective) },
        Err(_) => none,
    }
}

pub(crate) fn task_waiting_body(wait: &WaitRequest) -> String {
    json!({
        "kind": wait.kind,
        "resumeAt": wait.resume_at,
        "reason": wait.reason,
        "candidate": wait.candidate,
    }).to_string()
}

pub(crate) fn operational_failure_body(class: FailureClass, dispatch_id: &str, suggested: &CandidateKey) -> String {
    json!({
        "class": class,
        "dispatchId": dispatch_id,
        "suggested": suggested,
        "hint": "run retry",
    }).to_string()
}

pub(crate) struct NoticeInput<'a> {
    pub run_id: &'a str,
    pub task_id: &'a str,
    pub dispatch_id: Option<&'a str>,
    pub msg_type: &'a str,
    pub body: String,
    pub now: u64,
}

pub(crate) fn notice_cmd(entry: &RunEntry, input: NoticeInput) -> Option<Cmd> {
    let duplicate = entry.messages.iter().any(|m| {
        m.msg_type == input.msg_type
            && m.dispatch_id.as_deref() == input.dispatch_id
            && m.task_id.as_deref() == Some(input.task_id)
            && m.body.as_deref() == Some(input.body.as_str())
    });
    if duplicate {
        return None;
    }
    Some(Cmd::Send {
        run_id: input.run_id.to_string(),
        seq: entry.next_msg_seq(),
        from: "brain".into(),
        to: vec!["coordinator".into()],
        msg_type: input.msg_type.to_string(),
        task_id: Some(input.task_id.to_string()),
        dispatch_id: input.dispatch_id.map(str::to_string),
        reply_to: None,
        body: Some(input.body),
        outcome: None,
        files_modified: vec![],
        summary: None,
        now: input.now,
    })
}

#[cfg(windows)]
pub(crate) fn local_offset_min() -> i32 {
    #[repr(C)]
    struct TimeZoneInformation {
        bias: i32,
        standard_name: [u16; 32],
        standard_date: [u16; 8],
        standard_bias: i32,
        daylight_name: [u16; 32],
        daylight_date: [u16; 8],
        daylight_bias: i32,
    }
    extern "system" {
        fn GetTimeZoneInformation(info: *mut TimeZoneInformation) -> u32;
    }
    let mut info: TimeZoneInformation = unsafe { std::mem::zeroed() };
    let id = unsafe { GetTimeZoneInformation(&mut info) };
    let extra = match id {
        1 => info.standard_bias,
        2 => info.daylight_bias,
        _ => 0,
    };
    -(info.bias + extra)
}

#[cfg(unix)]
pub(crate) fn local_offset_min() -> i32 {
    unsafe {
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        libc::localtime_r(&now, &mut tm);
        (tm.tm_gmtoff / 60) as i32
    }
}
