use std::collections::BTreeMap;
use serde_json::{json, Value};
use crate::orch::model::{Dispatch, DispatchStatus, FailureClass, Task};
use crate::orch::reducer::RunEntry;
use super::usage::RunUsage;

const LOW_SAMPLE_BELOW: usize = 10;

#[derive(Default)]
struct Row {
    dispatches: usize,
    succeeded_reports: usize,
    verification_passed: usize,
    quality_failures: usize,
    operational_failures: BTreeMap<String, usize>,
    durations_ms: Vec<u64>,
    cost_usd: f64,
    cost_known: bool,
}

fn class_name(class: FailureClass) -> String {
    serde_json::to_value(class)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn verification_passed(d: &Dispatch) -> bool {
    !d.verifications.is_empty()
        && d.verifications.iter().all(|v| v.exit_code == Some(0) && !v.timed_out)
}

fn effective_failure(d: &Dispatch) -> Option<FailureClass> {
    if let Some(class) = d.failure_class {
        return Some(class);
    }
    let failed_report = d.outcome.as_deref() == Some("failed");
    let verification_failed = d.status == DispatchStatus::Reported
        && d.verifications.iter().any(|v| v.exit_code != Some(0) || v.timed_out);
    (failed_report || verification_failed).then_some(FailureClass::QualityFailed)
}

fn record(row: &mut Row, d: &Dispatch) {
    row.dispatches += 1;
    if d.outcome.as_deref() == Some("succeeded") {
        row.succeeded_reports += 1;
    }
    if verification_passed(d) {
        row.verification_passed += 1;
    }
    if let Some(class) = effective_failure(d) {
        match class.counter() {
            crate::orch::model::FailureCounter::Quality => row.quality_failures += 1,
            crate::orch::model::FailureCounter::Operational => {
                *row.operational_failures.entry(class_name(class)).or_default() += 1;
            }
            crate::orch::model::FailureCounter::None => {}
        }
    }
    if let Some(ended) = d.ended_at {
        row.durations_ms.push(ended.saturating_sub(d.started_at));
    }
    if let Some(cost) = d.cost_usd {
        row.cost_usd += cost;
        row.cost_known = true;
    }
}

fn median(values: &mut [u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[mid]
    } else {
        (values[mid - 1] + values[mid]) / 2
    })
}

fn role_of(task: Option<&Task>) -> String {
    task.and_then(|t| t.role.as_ref())
        .map(|r| r.as_str().to_string())
        .unwrap_or_else(|| "implement".to_string())
}

pub(super) struct MetricsSnapshot {
    rows: BTreeMap<(String, String, String), Row>,
    usage: Vec<RunUsage>,
}

pub(super) fn snapshot<'a>(runs: impl Iterator<Item = &'a RunEntry>) -> MetricsSnapshot {
    let mut rows: BTreeMap<(String, String, String), Row> = BTreeMap::new();
    let mut usage = Vec::new();
    for entry in runs {
        usage.push(RunUsage::snapshot(entry));
        for d in entry.dispatches.values() {
            let key = (
                d.profile.clone().unwrap_or_default(),
                d.model.clone().unwrap_or_default(),
                role_of(entry.tasks.get(d.task_id.as_str())),
            );
            record(rows.entry(key).or_default(), d);
        }
    }
    MetricsSnapshot { rows, usage }
}

pub(super) fn compute(snapshot: MetricsSnapshot) -> Value {
    let out: Vec<Value> = snapshot.rows.into_iter().map(|((agent, model, role), mut row)| {
        let median_ms = median(&mut row.durations_ms);
        json!({
            "agent": agent,
            "model": if model.is_empty() { Value::Null } else { Value::String(model) },
            "role": role,
            "dispatches": row.dispatches,
            "succeededReports": row.succeeded_reports,
            "verificationPassed": row.verification_passed,
            "qualityFailures": row.quality_failures,
            "operationalFailures": row.operational_failures,
            "medianDurationMs": median_ms,
            "costUsd": if row.cost_known { json!(row.cost_usd) } else { Value::Null },
            "lowSample": row.dispatches < LOW_SAMPLE_BELOW,
        })
    }).collect();
    let mut runs: Vec<_> = snapshot.usage.into_iter().map(RunUsage::read).collect();
    runs.sort_unstable_by(|a, b| a["runId"].as_str().cmp(&b["runId"].as_str()));
    json!({ "rows": out, "runs": runs })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::model::*;

    fn entry_with(dispatches: Vec<Dispatch>, role: Option<Role>) -> RunEntry {
        let run = Run {
            id: "r1".into(), title: "t".into(), repo: "/tmp".into(),
            base_ref: None, base_sha: None, setup: None, coordinator_tile: None,
            status: RunStatus::Active, limits: Limits::default(),
            autonomy: Autonomy::default(), detached: false, corrective_retry: true,
            worker_summary_cap: default_worker_summary_cap(), coordinator_wait: None, created_at: 0, updated_at: 0,
            allow_in_place: false, in_place_fingerprint: None, applied_integration_sha: None,
        };
        let mut e = RunEntry::new(run);
        e.tasks.insert("t1".into(), Task { owner_reason: None,
            id: "t1".into(), title: "t".into(), spec: "s".into(), acceptance: None,
            deps: vec![], profile: None, escalation: vec![], verify: vec![], exclusive: vec![], role, review: false,
            owner: None, status: TaskStatus::Pending, result: None, integrated_sha: None, attempts: 0,
            routing: 1, quality_floor: 0, quality_attempts: 0, operational_attempts: 0,
            pinned: false, last_failure: None, wait: None, created_at: 0, updated_at: 0,
            workspace: None, applied_in_place: false, timeout_min: None, max_cost_usd: None,
        });
        for d in dispatches {
            e.dispatches.insert(d.id.clone(), d);
        }
        e
    }

    fn disp(id: &str, outcome: Option<&str>, class: Option<FailureClass>, dur: u64, cost: Option<f64>) -> Dispatch {
        Dispatch { merge_branch: None, report: None,
            id: id.into(), task_id: "t1".into(), attempt: 1, retry_of: None,
            profile: Some("claude".into()), model: Some("opus".into()), effort: None, tile_id: None,
            worktree: None, branch: None, status: DispatchStatus::Reported,
            outcome: outcome.map(str::to_string), head_sha: None, files_modified: vec![],
            summary: None, started_at: 100, ended_at: Some(100 + dur), last_heartbeat: None,
            verifications: vec![], cost_usd: cost, cost_source: None,
            account: None, chain_index: None, trigger: None, decision: None,
            failure_class: class, by: None, start_ref: None,
            native_home: None, native_session: None, corrective_retry: false, retry_same_entry: false, operational_wait: false,
            diff_stat: None,
        }
    }

    #[test]
    fn groups_counts_and_flags_low_sample() {
        let e = entry_with(vec![
            disp("a", Some("succeeded"), None, 1000, Some(0.5)),
            disp("b", Some("failed"), None, 3000, Some(0.25)),
            disp("c", None, Some(FailureClass::RateLimited), 2000, None),
            disp("d", None, Some(FailureClass::RateLimited), 2000, None),
            disp("e", None, Some(FailureClass::QuotaExhausted), 2000, None),
        ], Some(Role::Review));
        let v = compute(snapshot([&e].into_iter()));
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        let r = &rows[0];
        assert_eq!(r["agent"], "claude");
        assert_eq!(r["model"], "opus");
        assert_eq!(r["role"], "review");
        assert_eq!(r["dispatches"], 5);
        assert_eq!(r["succeededReports"], 1);
        assert_eq!(r["qualityFailures"], 1);
        assert_eq!(r["operationalFailures"]["rate_limited"], 2);
        assert_eq!(r["operationalFailures"]["quota_exhausted"], 1);
        assert_eq!(r["medianDurationMs"], 2000);
        assert_eq!(r["costUsd"], 0.75);
        assert_eq!(r["lowSample"], true);
        let run = &v["runs"][0];
        assert_eq!(run["runId"], "r1");
        assert_eq!(run["coordinator"]["tokens"]["input"], json!({"value": null, "provenance": "unavailable"}));
        assert_eq!(run["workersByProfile"]["claude"]["input"], json!({"value": null, "provenance": "unavailable"}));
    }

    #[test]
    fn ten_dispatches_is_not_low_sample_and_no_cost_is_null() {
        let ds: Vec<Dispatch> = (0..10).map(|i| disp(&format!("d{i}"), Some("succeeded"), None, 10, None)).collect();
        let e = entry_with(ds, None);
        let v = compute(snapshot([&e].into_iter()));
        let r = &v["rows"][0];
        assert_eq!(r["role"], "implement");
        assert_eq!(r["lowSample"], false);
        assert!(r["costUsd"].is_null());
    }

    #[test]
    fn empty_state_has_no_rows() {
        let v = compute(snapshot(std::iter::empty()));
        assert_eq!(v["rows"].as_array().unwrap().len(), 0);
        assert_eq!(v["runs"], json!([]));
    }
}
