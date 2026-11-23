use crate::orch::candidates::{CandidateKey, CandidateState, CandidateStates, CandidateStatus};
use crate::orch::model::{SkippedCandidate, WaitKind};

pub const DEFAULT_SHORT_WAIT_MS: i64 = 10 * 60_000;

pub struct RouteInput<'a> {
    pub chain: &'a [CandidateKey],
    pub floor: u32,
    pub pinned: Option<&'a CandidateKey>,
    pub states: &'a CandidateStates,
    pub now: i64,
    pub short_wait_ms: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RouteOutcome {
    Dispatch {
        candidate: CandidateKey,
        chain_index: Option<u32>,
        reason: String,
        skipped: Vec<SkippedCandidate>,
    },
    Wait {
        kind: WaitKind,
        resume_at: Option<i64>,
        candidate: Option<CandidateKey>,
        reason: String,
        skipped: Vec<SkippedCandidate>,
    },
}

pub fn floor_after_quality_failure(floor: u32, chain_index: Option<u32>) -> u32 {
    match chain_index {
        Some(i) => floor.max(i.saturating_add(1)),
        None => floor,
    }
}

fn wait_kind(state: &CandidateState) -> WaitKind {
    if state.state == CandidateStatus::AuthFailed || state.until.is_none() {
        return WaitKind::Intervention;
    }
    if state.state == CandidateStatus::QuotaExhausted {
        WaitKind::Quota
    } else {
        WaitKind::Availability
    }
}

fn blocked_reason(key: &CandidateKey, state: &CandidateState) -> String {
    format!("{} {}", key.label(), state.state.as_str())
}

pub fn route(input: RouteInput<'_>) -> RouteOutcome {
    let RouteInput { chain, floor, pinned, states, now, short_wait_ms } = input;

    if let Some(pinned) = pinned {
        let state = states.lookup(pinned, now);
        if state.state.is_eligible() {
            return RouteOutcome::Dispatch {
                candidate: pinned.clone(),
                chain_index: None,
                reason: "pinned".into(),
                skipped: vec![],
            };
        }
        return RouteOutcome::Wait {
            kind: wait_kind(&state),
            resume_at: state.until,
            candidate: Some(pinned.clone()),
            reason: format!("pinned: {}", blocked_reason(pinned, &state)),
            skipped: vec![],
        };
    }

    if chain.is_empty() {
        return RouteOutcome::Wait {
            kind: WaitKind::Intervention,
            resume_at: None,
            candidate: None,
            reason: "no candidates in the role chain".into(),
            skipped: vec![],
        };
    }

    let start = (floor as usize).min(chain.len() - 1);
    let window = &chain[start..];
    let head = &window[0];
    let head_state = states.lookup(head, now);

    if !head_state.state.is_eligible() {
        if let Some(until) = head_state.until.filter(|u| *u <= now.saturating_add(short_wait_ms)) {
            return RouteOutcome::Wait {
                kind: wait_kind(&head_state),
                resume_at: Some(until),
                candidate: Some(head.clone()),
                reason: format!("short wait: {}", blocked_reason(head, &head_state)),
                skipped: vec![],
            };
        }
    }

    let mut skipped: Vec<SkippedCandidate> = Vec::new();
    let mut earliest: Option<(i64, CandidateKey, CandidateState)> = None;
    for (offset, candidate) in window.iter().enumerate() {
        let state = if offset == 0 { head_state.clone() } else { states.lookup(candidate, now) };
        if state.state.is_eligible() {
            let reason = if offset == 0 {
                "preferred model available".to_string()
            } else {
                format!("fallback: {}", blocked_reason(head, &head_state))
            };
            return RouteOutcome::Dispatch {
                candidate: candidate.clone(),
                chain_index: Some((start + offset) as u32),
                reason,
                skipped,
            };
        }
        if let Some(until) = state.until {
            if earliest.as_ref().map(|(u, _, _)| until < *u).unwrap_or(true) {
                earliest = Some((until, candidate.clone(), state.clone()));
            }
        }
        skipped.push(SkippedCandidate {
            candidate: candidate.clone(),
            state: state.state,
            reason: blocked_reason(candidate, &state),
        });
    }

    match earliest {
        Some((until, candidate, state)) => RouteOutcome::Wait {
            kind: wait_kind(&state),
            resume_at: Some(until),
            reason: format!("no eligible candidate; earliest: {}", blocked_reason(&candidate, &state)),
            candidate: Some(candidate),
            skipped,
        },
        None => RouteOutcome::Wait {
            kind: WaitKind::Intervention,
            resume_at: None,
            candidate: None,
            reason: "no eligible candidate and no known reset time".into(),
            skipped,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::candidates::{Confidence, MarkInput};
    use crate::orch::model::FailureClass;

    const NOW: i64 = 1_000_000_000;
    const HOUR: i64 = 3_600_000;

    fn k(entry: &str) -> CandidateKey {
        CandidateKey::from_chain_entry(entry)
    }

    fn chain(entries: &[&str]) -> Vec<CandidateKey> {
        entries.iter().map(|e| k(e)).collect()
    }

    fn block(states: &mut CandidateStates, entry: &str, class: FailureClass, reset_at: Option<i64>) {
        assert!(states.mark(MarkInput {
            key: &k(entry), class, reset_at, source: "classifier", confidence: Confidence::Reliable, now: NOW,
        }));
    }

    fn run(chain: &[CandidateKey], floor: u32, pinned: Option<&CandidateKey>, states: &CandidateStates) -> RouteOutcome {
        route(RouteInput { chain, floor, pinned, states, now: NOW, short_wait_ms: DEFAULT_SHORT_WAIT_MS })
    }

    fn dispatched(outcome: RouteOutcome) -> (CandidateKey, Option<u32>, String, Vec<SkippedCandidate>) {
        match outcome {
            RouteOutcome::Dispatch { candidate, chain_index, reason, skipped } => (candidate, chain_index, reason, skipped),
            other => panic!("expected dispatch, got {other:?}"),
        }
    }

    fn waited(outcome: RouteOutcome) -> (WaitKind, Option<i64>, Option<CandidateKey>, String) {
        match outcome {
            RouteOutcome::Wait { kind, resume_at, candidate, reason, .. } => (kind, resume_at, candidate, reason),
            other => panic!("expected wait, got {other:?}"),
        }
    }

    #[test]
    fn preference_order_picks_head_when_eligible() {
        let c = chain(&["a", "b", "c"]);
        let (cand, idx, reason, skipped) = dispatched(run(&c, 0, None, &CandidateStates::default()));
        assert_eq!(cand, k("a"));
        assert_eq!(idx, Some(0));
        assert_eq!(reason, "preferred model available");
        assert!(skipped.is_empty());
    }

    #[test]
    fn blocked_head_falls_back_with_skipped_record() {
        let c = chain(&["a", "b", "c"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW + HOUR));
        let (cand, idx, reason, skipped) = dispatched(run(&c, 0, None, &states));
        assert_eq!(cand, k("b"));
        assert_eq!(idx, Some(1));
        assert_eq!(reason, "fallback: a rate_limited");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].candidate, k("a"));
        assert_eq!(skipped[0].state, CandidateStatus::RateLimited);
    }

    #[test]
    fn skips_multiple_blocked_entries() {
        let c = chain(&["a", "b", "c"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW + HOUR));
        block(&mut states, "b", FailureClass::AuthFailed, None);
        let (cand, idx, _, skipped) = dispatched(run(&c, 0, None, &states));
        assert_eq!(cand, k("c"));
        assert_eq!(idx, Some(2));
        assert_eq!(skipped.len(), 2);
    }

    #[test]
    fn unknown_state_counts_as_eligible() {
        let c = chain(&["a"]);
        let (cand, _, reason, _) = dispatched(run(&c, 0, None, &CandidateStates::default()));
        assert_eq!(cand, k("a"));
        assert_eq!(reason, "preferred model available");
    }

    #[test]
    fn expired_block_counts_as_eligible() {
        let c = chain(&["a", "b"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW - 1));
        let (cand, _, _, _) = dispatched(run(&c, 0, None, &states));
        assert_eq!(cand, k("a"));
    }

    #[test]
    fn pinned_eligible_dispatches_without_chain_index() {
        let c = chain(&["a", "b"]);
        let pinned = k("p:m");
        let (cand, idx, reason, _) = dispatched(run(&c, 1, Some(&pinned), &CandidateStates::default()));
        assert_eq!(cand, pinned);
        assert_eq!(idx, None);
        assert_eq!(reason, "pinned");
    }

    #[test]
    fn pinned_blocked_waits_and_never_substitutes() {
        let c = chain(&["a", "b"]);
        let pinned = k("p");
        let mut states = CandidateStates::default();
        block(&mut states, "p", FailureClass::QuotaExhausted, Some(NOW + 5 * HOUR));
        let (kind, resume, cand, _) = waited(run(&c, 0, Some(&pinned), &states));
        assert_eq!(kind, WaitKind::Quota);
        assert_eq!(resume, Some(NOW + 5 * HOUR));
        assert_eq!(cand, Some(pinned));
    }

    #[test]
    fn pinned_auth_failed_needs_intervention() {
        let pinned = k("p");
        let mut states = CandidateStates::default();
        block(&mut states, "p", FailureClass::AuthFailed, None);
        let (kind, resume, _, _) = waited(run(&[], 0, Some(&pinned), &states));
        assert_eq!(kind, WaitKind::Intervention);
        assert_eq!(resume, None);
    }

    #[test]
    fn floor_excludes_earlier_entries() {
        let c = chain(&["a", "b", "c"]);
        let (cand, idx, reason, _) = dispatched(run(&c, 1, None, &CandidateStates::default()));
        assert_eq!(cand, k("b"));
        assert_eq!(idx, Some(1));
        assert_eq!(reason, "preferred model available");
    }

    #[test]
    fn floor_never_returns_to_rejected_entry_even_when_later_ones_blocked() {
        let c = chain(&["a", "b", "c"]);
        let mut states = CandidateStates::default();
        block(&mut states, "b", FailureClass::RateLimited, Some(NOW + 2 * HOUR));
        block(&mut states, "c", FailureClass::RateLimited, Some(NOW + 3 * HOUR));
        let (_, resume, cand, _) = waited(run(&c, 1, None, &states));
        assert_eq!(resume, Some(NOW + 2 * HOUR));
        assert_eq!(cand, Some(k("b")));
    }

    #[test]
    fn floor_past_end_uses_last_entry() {
        let c = chain(&["a", "b", "c"]);
        let (cand, idx, _, _) = dispatched(run(&c, 7, None, &CandidateStates::default()));
        assert_eq!(cand, k("c"));
        assert_eq!(idx, Some(2));
    }

    #[test]
    fn short_wait_keeps_preferred_model() {
        let c = chain(&["a", "b"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW + 5 * 60_000));
        let (kind, resume, cand, _) = waited(run(&c, 0, None, &states));
        assert_eq!(kind, WaitKind::Availability);
        assert_eq!(resume, Some(NOW + 5 * 60_000));
        assert_eq!(cand, Some(k("a")));
    }

    #[test]
    fn short_wait_boundary_is_inclusive() {
        let c = chain(&["a", "b"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW + DEFAULT_SHORT_WAIT_MS));
        assert!(matches!(run(&c, 0, None, &states), RouteOutcome::Wait { .. }));
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW + DEFAULT_SHORT_WAIT_MS + 1));
        assert!(matches!(run(&c, 0, None, &states), RouteOutcome::Dispatch { .. }));
    }

    #[test]
    fn short_wait_applies_to_window_head_not_chain_head() {
        let c = chain(&["a", "b", "c"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::RateLimited, Some(NOW + 60_000));
        let (cand, _, _, _) = dispatched(run(&c, 1, None, &states));
        assert_eq!(cand, k("b"));
    }

    #[test]
    fn wait_picks_earliest_known_reset_quota_kind() {
        let c = chain(&["a", "b"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::QuotaExhausted, Some(NOW + 5 * HOUR));
        block(&mut states, "b", FailureClass::QuotaExhausted, Some(NOW + 2 * HOUR));
        let (kind, resume, cand, _) = waited(run(&c, 0, None, &states));
        assert_eq!(kind, WaitKind::Quota);
        assert_eq!(resume, Some(NOW + 2 * HOUR));
        assert_eq!(cand, Some(k("b")));
    }

    #[test]
    fn wait_kind_is_availability_when_earliest_is_rate_limited() {
        let c = chain(&["a", "b"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::QuotaExhausted, Some(NOW + 5 * HOUR));
        block(&mut states, "b", FailureClass::RateLimited, Some(NOW + HOUR));
        let (kind, resume, _, _) = waited(run(&c, 0, None, &states));
        assert_eq!(kind, WaitKind::Availability);
        assert_eq!(resume, Some(NOW + HOUR));
    }

    #[test]
    fn wait_without_known_reset_needs_intervention() {
        let c = chain(&["a", "b"]);
        let mut states = CandidateStates::default();
        block(&mut states, "a", FailureClass::QuotaExhausted, None);
        block(&mut states, "b", FailureClass::AuthFailed, None);
        let (kind, resume, cand, _) = waited(run(&c, 0, None, &states));
        assert_eq!(kind, WaitKind::Intervention);
        assert_eq!(resume, None);
        assert_eq!(cand, None);
    }

    #[test]
    fn empty_chain_without_pin_needs_intervention() {
        let (kind, _, _, _) = waited(run(&[], 0, None, &CandidateStates::default()));
        assert_eq!(kind, WaitKind::Intervention);
    }

    #[test]
    fn floor_helper_only_moves_forward_and_ignores_pinned() {
        assert_eq!(floor_after_quality_failure(0, Some(0)), 1);
        assert_eq!(floor_after_quality_failure(3, Some(0)), 3);
        assert_eq!(floor_after_quality_failure(1, Some(2)), 3);
        assert_eq!(floor_after_quality_failure(2, None), 2);
    }

    #[test]
    fn worked_scenario_from_spec() {
        let c = chain(&["a", "b", "c"]);
        let mut states = CandidateStates::default();
        let mut floor = floor_after_quality_failure(0, Some(0));
        assert_eq!(floor, 1);
        block(&mut states, "b", FailureClass::RateLimited, Some(NOW + 2 * HOUR));
        block(&mut states, "c", FailureClass::QuotaExhausted, Some(NOW + 4 * HOUR));
        let (kind, resume, cand, _) = waited(run(&c, floor, None, &states));
        assert_eq!(kind, WaitKind::Availability);
        assert_eq!(resume, Some(NOW + 2 * HOUR));
        assert_eq!(cand, Some(k("b")));

        let mut no_reset = CandidateStates::default();
        block(&mut no_reset, "b", FailureClass::AuthFailed, None);
        block(&mut no_reset, "c", FailureClass::QuotaExhausted, None);
        let (kind, _, _, _) = waited(run(&c, floor, None, &no_reset));
        assert_eq!(kind, WaitKind::Intervention);

        let mut only_b_blocked = CandidateStates::default();
        block(&mut only_b_blocked, "b", FailureClass::RateLimited, Some(NOW + 2 * HOUR));
        let (cand, idx, reason, _) = dispatched(run(&c, floor, None, &only_b_blocked));
        assert_eq!(cand, k("c"));
        assert_eq!(idx, Some(2));
        assert_eq!(reason, "fallback: b rate_limited");

        floor = floor_after_quality_failure(floor, idx);
        assert_eq!(floor, 3);
        let (cand, _, _, _) = dispatched(run(&c, floor, None, &CandidateStates::default()));
        assert_eq!(cand, k("c"));
    }
}
