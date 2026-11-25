use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::orch::model::FailureClass;

pub const BACKOFF_STEPS_MS: [i64; 3] = [2 * 60_000, 10 * 60_000, 30 * 60_000];
pub const USAGE_SNAPSHOT_SOURCE: &str = "usage-snapshot";
pub const USAGE_RESERVE_PERCENT: f64 = 90.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub struct CandidateKey {
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

impl CandidateKey {
    pub fn from_chain_entry(entry: &str) -> Self {
        let (agent, model) = entry.split_once(':')
            .map(|(p, m)| (p.to_string(), Some(m.to_string())))
            .unwrap_or_else(|| (entry.to_string(), None));
        Self { agent, model, account: None }
    }

    pub fn label(&self) -> String {
        match &self.model {
            Some(m) => format!("{}:{}", self.agent, m),
            None => self.agent.clone(),
        }
    }

    fn covers(&self, other: &CandidateKey) -> bool {
        self.agent == other.agent
            && (self.model.is_none() || self.model == other.model)
            && (self.account.is_none() || other.account.is_none() || self.account == other.account)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Available,
    RateLimited,
    QuotaExhausted,
    AuthFailed,
    Unavailable,
    #[default]
    Unknown,
}

impl CandidateStatus {
    pub fn is_eligible(self) -> bool {
        matches!(self, CandidateStatus::Available | CandidateStatus::Unknown)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CandidateStatus::Available => "available",
            CandidateStatus::RateLimited => "rate_limited",
            CandidateStatus::QuotaExhausted => "quota_exhausted",
            CandidateStatus::AuthFailed => "auth_failed",
            CandidateStatus::Unavailable => "unavailable",
            CandidateStatus::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    #[default]
    Reliable,
    Partial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CandidateState {
    #[serde(default)]
    pub state: CandidateStatus,
    #[serde(default)]
    pub until: Option<i64>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub confidence: Confidence,
    #[serde(default)]
    pub updated_at: i64,
    #[serde(default)]
    pub consecutive: u32,
}

impl CandidateState {
    pub fn unknown() -> Self {
        Self {
            state: CandidateStatus::Unknown,
            until: None,
            source: String::new(),
            confidence: Confidence::Partial,
            updated_at: 0,
            consecutive: 0,
        }
    }

    fn is_blocking_at(&self, now: i64) -> bool {
        if self.state.is_eligible() {
            return false;
        }
        self.until.map(|u| u > now).unwrap_or(true)
    }

    fn until_or_max(&self) -> i64 {
        self.until.unwrap_or(i64::MAX)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateEntry {
    pub key: CandidateKey,
    #[serde(flatten)]
    pub state: CandidateState,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CandidateStates {
    #[serde(default)]
    entries: Vec<CandidateEntry>,
}

pub struct MarkInput<'a> {
    pub key: &'a CandidateKey,
    pub class: FailureClass,
    pub reset_at: Option<i64>,
    pub source: &'a str,
    pub confidence: Confidence,
    pub now: i64,
}

impl CandidateStates {
    pub fn lookup(&self, key: &CandidateKey, now: i64) -> CandidateState {
        let matching: Vec<&CandidateEntry> = self.entries.iter()
            .filter(|e| e.key.covers(key))
            .collect();
        if let Some(blocking) = matching.iter()
            .filter(|e| e.state.is_blocking_at(now))
            .max_by_key(|e| e.state.until_or_max())
        {
            return blocking.state.clone();
        }
        match matching.into_iter().max_by_key(|e| e.state.updated_at) {
            Some(e) if e.state.state == CandidateStatus::Unknown => e.state.clone(),
            Some(e) => CandidateState {
                state: CandidateStatus::Available,
                until: None,
                source: e.state.source.clone(),
                confidence: e.state.confidence,
                updated_at: e.state.updated_at,
                consecutive: e.state.consecutive,
            },
            None => CandidateState::unknown(),
        }
    }

    pub fn entries(&self, now: i64) -> Vec<CandidateEntry> {
        self.entries.iter()
            .filter(|e| e.state.is_blocking_at(now))
            .cloned()
            .collect()
    }

    pub fn mark(&mut self, input: MarkInput<'_>) -> bool {
        let MarkInput { key, class, reset_at, source, confidence, now } = input;
        let previous = self.entries.iter().find(|e| e.key == *key).map(|e| e.state.clone());
        let prev_consecutive = previous.as_ref().map(|p| p.consecutive).unwrap_or(0);
        let (state, until, consecutive) = match class {
            FailureClass::QuotaExhausted => (CandidateStatus::QuotaExhausted, reset_at, 0),
            FailureClass::RateLimited => (CandidateStatus::RateLimited, reset_at, 0),
            FailureClass::AuthFailed => (CandidateStatus::AuthFailed, None, 0),
            FailureClass::ModelUnavailable => (CandidateStatus::Unavailable, reset_at, 0),
            FailureClass::LaunchFailed => {
                let consecutive = prev_consecutive.saturating_add(1);
                let step = BACKOFF_STEPS_MS[(consecutive as usize - 1).min(BACKOFF_STEPS_MS.len() - 1)];
                (CandidateStatus::Unavailable, Some(now + step), consecutive)
            }
            _ => return false,
        };
        self.upsert(CandidateEntry {
            key: key.clone(),
            state: CandidateState { state, until, source: source.to_string(), confidence, updated_at: now, consecutive },
        });
        true
    }

    fn upsert(&mut self, entry: CandidateEntry) {
        match self.entries.iter_mut().find(|e| e.key == entry.key) {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
    }

    pub fn clear(&mut self, filter: &CandidateKey) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !(
            e.key.agent == filter.agent
                && (filter.model.is_none() || e.key.model == filter.model)
                && (filter.account.is_none() || e.key.account == filter.account)
        ));
        before - self.entries.len()
    }

    pub fn clear_on_success(&mut self, key: &CandidateKey) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !e.key.covers(key));
        before - self.entries.len()
    }

    pub fn clear_usage_snapshot(&mut self, agent: &str, account: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|e| !(
            e.state.source == USAGE_SNAPSHOT_SOURCE
                && e.key.agent == agent
                && e.key.account.as_deref() == Some(account)
        ));
        before - self.entries.len()
    }

    pub fn mark_usage_exhausted(&mut self, agent: &str, account: &str, resets_at: Option<i64>, now: i64) {
        let key = CandidateKey { agent: agent.to_string(), model: None, account: Some(account.to_string()) };
        self.upsert(CandidateEntry {
            key,
            state: CandidateState {
                state: CandidateStatus::QuotaExhausted,
                until: resets_at,
                source: USAGE_SNAPSHOT_SOURCE.to_string(),
                confidence: Confidence::Reliable,
                updated_at: now,
                consecutive: 0,
            },
        });
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    #[serde(default)]
    pub key: String,
    pub used: f64,
    #[serde(default)]
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub agent: String,
    pub account_id: String,
    #[serde(default)]
    pub windows: Vec<UsageWindow>,
}

impl CandidateStates {
    pub fn apply_usage_report(&mut self, report: &UsageReport, now: i64) {
        if report.windows.is_empty() {
            return;
        }
        let exhausted: Vec<&UsageWindow> = report.windows.iter().filter(|w| w.used >= USAGE_RESERVE_PERCENT).collect();
        if exhausted.is_empty() {
            self.clear_usage_snapshot(&report.agent, &report.account_id);
            return;
        }
        let until = exhausted.iter()
            .map(|w| w.resets_at)
            .collect::<Option<Vec<i64>>>()
            .and_then(|all| all.into_iter().max());
        self.mark_usage_exhausted(&report.agent, &report.account_id, until, now);
    }
}

pub fn candidates_path(base: &Path) -> PathBuf {
    base.join("orch").join("candidates.json")
}

pub fn load(base: &Path) -> CandidateStates {
    std::fs::read_to_string(candidates_path(base))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(base: &Path, states: &CandidateStates) -> Result<(), String> {
    let json = serde_json::to_string(states).map_err(|e| e.to_string())?;
    let path = candidates_path(base);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, &json).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_000_000_000;

    fn key(agent: &str, model: Option<&str>) -> CandidateKey {
        CandidateKey { agent: agent.into(), model: model.map(str::to_string), account: None }
    }

    fn mark(states: &mut CandidateStates, k: &CandidateKey, class: FailureClass, reset_at: Option<i64>, now: i64) {
        assert!(states.mark(MarkInput {
            key: k, class, reset_at, source: "classifier", confidence: Confidence::Reliable, now,
        }));
    }

    #[test]
    fn chain_entry_splits_on_first_colon() {
        let k = CandidateKey::from_chain_entry("opencode:openai/gpt-5.5");
        assert_eq!(k.agent, "opencode");
        assert_eq!(k.model.as_deref(), Some("openai/gpt-5.5"));
        let k = CandidateKey::from_chain_entry("claude");
        assert_eq!(k.model, None);
    }

    #[test]
    fn unknown_when_no_entry() {
        let states = CandidateStates::default();
        assert_eq!(states.lookup(&key("claude", None), NOW).state, CandidateStatus::Unknown);
    }

    #[test]
    fn entry_expires_lazily() {
        let mut states = CandidateStates::default();
        let k = key("claude", Some("opus"));
        mark(&mut states, &k, FailureClass::RateLimited, Some(NOW + 1000), NOW);
        assert_eq!(states.lookup(&k, NOW).state, CandidateStatus::RateLimited);
        assert_eq!(states.lookup(&k, NOW + 999).state, CandidateStatus::RateLimited);
        assert_eq!(states.lookup(&k, NOW + 1000).state, CandidateStatus::Available);
        assert!(states.entries(NOW + 1000).is_empty());
    }

    #[test]
    fn quota_without_until_stays_blocked() {
        let mut states = CandidateStates::default();
        let k = key("claude", None);
        mark(&mut states, &k, FailureClass::QuotaExhausted, None, NOW);
        assert_eq!(states.lookup(&k, NOW + 10_000_000_000).state, CandidateStatus::QuotaExhausted);
    }

    #[test]
    fn account_wide_entry_matches_every_model() {
        let mut states = CandidateStates::default();
        mark(&mut states, &key("claude", None), FailureClass::QuotaExhausted, Some(NOW + 5000), NOW);
        assert_eq!(states.lookup(&key("claude", Some("opus")), NOW).state, CandidateStatus::QuotaExhausted);
        assert_eq!(states.lookup(&key("claude", Some("sonnet")), NOW).state, CandidateStatus::QuotaExhausted);
        assert_eq!(states.lookup(&key("codex", None), NOW).state, CandidateStatus::Unknown);
    }

    #[test]
    fn model_scoped_entry_does_not_match_other_models() {
        let mut states = CandidateStates::default();
        mark(&mut states, &key("claude", Some("opus")), FailureClass::RateLimited, Some(NOW + 5000), NOW);
        assert_eq!(states.lookup(&key("claude", Some("opus")), NOW).state, CandidateStatus::RateLimited);
        assert_eq!(states.lookup(&key("claude", Some("sonnet")), NOW).state, CandidateStatus::Unknown);
        assert_eq!(states.lookup(&key("claude", None), NOW).state, CandidateStatus::Unknown);
    }

    #[test]
    fn account_scoped_entry_matches_default_and_same_account_only() {
        let mut states = CandidateStates::default();
        states.mark_usage_exhausted("claude", "acc1", Some(NOW + 5000), NOW);
        let same = CandidateKey { agent: "claude".into(), model: None, account: Some("acc1".into()) };
        let other = CandidateKey { agent: "claude".into(), model: None, account: Some("acc2".into()) };
        assert_eq!(states.lookup(&same, NOW).state, CandidateStatus::QuotaExhausted);
        assert_eq!(states.lookup(&other, NOW).state, CandidateStatus::Unknown);
        assert_eq!(states.lookup(&key("claude", None), NOW).state, CandidateStatus::QuotaExhausted);
    }

    #[test]
    fn launch_failure_backoff_sequence() {
        let mut states = CandidateStates::default();
        let k = key("claude", Some("opus"));
        let mut untils = vec![];
        for i in 0..5 {
            let now = NOW + i * 100;
            mark(&mut states, &k, FailureClass::LaunchFailed, None, now);
            let st = states.lookup(&k, now);
            assert_eq!(st.state, CandidateStatus::Unavailable);
            untils.push(st.until.unwrap() - now);
        }
        assert_eq!(untils, vec![120_000, 600_000, 1_800_000, 1_800_000, 1_800_000]);
    }

    #[test]
    fn non_candidate_classes_do_not_mark() {
        let mut states = CandidateStates::default();
        let k = key("claude", None);
        for class in [FailureClass::ExecutionFailed, FailureClass::SetupFailed, FailureClass::QualityFailed, FailureClass::Unknown] {
            assert!(!states.mark(MarkInput {
                key: &k, class, reset_at: None, source: "x", confidence: Confidence::Reliable, now: NOW,
            }));
        }
        assert_eq!(states.lookup(&k, NOW).state, CandidateStatus::Unknown);
    }

    #[test]
    fn auth_failed_never_expires_and_is_not_quota() {
        let mut states = CandidateStates::default();
        let k = key("codex", None);
        mark(&mut states, &k, FailureClass::AuthFailed, Some(NOW + 1), NOW);
        let st = states.lookup(&k, NOW + 1_000_000);
        assert_eq!(st.state, CandidateStatus::AuthFailed);
        assert_eq!(st.until, None);
    }

    #[test]
    fn clear_on_success_removes_matching_and_resets_consecutive() {
        let mut states = CandidateStates::default();
        let k = key("claude", Some("opus"));
        mark(&mut states, &k, FailureClass::LaunchFailed, None, NOW);
        mark(&mut states, &key("claude", None), FailureClass::RateLimited, Some(NOW + 9999), NOW);
        mark(&mut states, &key("codex", None), FailureClass::RateLimited, Some(NOW + 9999), NOW);
        assert_eq!(states.clear_on_success(&k), 2);
        assert_eq!(states.lookup(&k, NOW).state, CandidateStatus::Unknown);
        assert_eq!(states.lookup(&key("codex", None), NOW).state, CandidateStatus::RateLimited);
        mark(&mut states, &k, FailureClass::LaunchFailed, None, NOW);
        assert_eq!(states.lookup(&k, NOW).consecutive, 1);
    }

    #[test]
    fn clear_filters_by_agent_model_and_account() {
        let mut states = CandidateStates::default();
        mark(&mut states, &key("claude", Some("opus")), FailureClass::RateLimited, None, NOW);
        mark(&mut states, &key("claude", Some("sonnet")), FailureClass::RateLimited, None, NOW);
        mark(&mut states, &key("codex", None), FailureClass::RateLimited, None, NOW);
        assert_eq!(states.clear(&key("claude", Some("opus"))), 1);
        assert_eq!(states.entries(NOW).len(), 2);
        assert_eq!(states.clear(&key("claude", None)), 1);
        assert_eq!(states.entries(NOW).len(), 1);
    }

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("panorama-cand-{}-{}", std::process::id(), NOW));
        let _ = std::fs::remove_dir_all(&dir);
        let mut states = CandidateStates::default();
        mark(&mut states, &key("claude", Some("opus")), FailureClass::QuotaExhausted, Some(NOW + 5000), NOW);
        states.mark_usage_exhausted("codex", "a1", None, NOW);
        save(&dir, &states).unwrap();
        assert!(candidates_path(&dir).exists());
        assert!(!candidates_path(&dir).with_extension("json.tmp").exists());
        let loaded = load(&dir);
        assert_eq!(loaded.entries(NOW), states.entries(NOW));
        assert_eq!(loaded.entries(NOW).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn report(windows: Vec<(f64, Option<i64>)>) -> UsageReport {
        UsageReport {
            agent: "claude".into(),
            account_id: "acc1".into(),
            windows: windows.into_iter().enumerate()
                .map(|(i, (used, resets_at))| UsageWindow { key: format!("w{i}"), used, resets_at })
                .collect(),
        }
    }

    #[test]
    fn usage_report_exhausted_sets_quota_until_reset() {
        let mut states = CandidateStates::default();
        states.apply_usage_report(&report(vec![(40.0, Some(NOW + 1)), (100.0, Some(NOW + 5000))]), NOW);
        let k = CandidateKey { agent: "claude".into(), model: None, account: Some("acc1".into()) };
        let st = states.lookup(&k, NOW);
        assert_eq!(st.state, CandidateStatus::QuotaExhausted);
        assert_eq!(st.until, Some(NOW + 5000));
        assert_eq!(st.source, USAGE_SNAPSHOT_SOURCE);
        assert_eq!(st.confidence, Confidence::Reliable);
    }

    #[test]
    fn usage_report_over_100_counts_and_unknown_reset_stays_open() {
        let mut states = CandidateStates::default();
        states.apply_usage_report(&report(vec![(120.0, Some(NOW + 5000)), (100.0, None)]), NOW);
        let k = key("claude", None);
        let st = states.lookup(&k, NOW + 1_000_000);
        assert_eq!(st.state, CandidateStatus::QuotaExhausted);
        assert_eq!(st.until, None);
    }

    #[test]
    fn usage_report_reserve_blocks_before_100() {
        let mut states = CandidateStates::default();
        states.apply_usage_report(&report(vec![(89.9, Some(NOW + 1))]), NOW);
        assert!(states.entries(NOW).is_empty());
        states.apply_usage_report(&report(vec![(90.0, Some(NOW + 5000))]), NOW);
        let st = states.lookup(&key("claude", None), NOW);
        assert_eq!(st.state, CandidateStatus::QuotaExhausted);
        assert_eq!(st.until, Some(NOW + 5000));
    }

    #[test]
    fn usage_report_below_reserve_clears_only_snapshot_entries() {
        let mut states = CandidateStates::default();
        states.apply_usage_report(&report(vec![(100.0, Some(NOW + 5000))]), NOW);
        mark(&mut states, &key("claude", Some("opus")), FailureClass::RateLimited, Some(NOW + 9000), NOW);
        let classifier_quota = CandidateKey { agent: "claude".into(), model: None, account: Some("other".into()) };
        mark(&mut states, &classifier_quota, FailureClass::QuotaExhausted, None, NOW);
        assert_eq!(states.entries(NOW).len(), 3);
        states.apply_usage_report(&report(vec![(10.0, Some(NOW + 5000)), (89.9, None)]), NOW + 1);
        let left = states.entries(NOW + 1);
        assert_eq!(left.len(), 2);
        assert!(left.iter().all(|e| e.state.source == "classifier"));
    }

    #[test]
    fn usage_report_without_windows_changes_nothing() {
        let mut states = CandidateStates::default();
        states.apply_usage_report(&report(vec![(100.0, None)]), NOW);
        states.apply_usage_report(&report(vec![]), NOW + 1);
        assert_eq!(states.entries(NOW + 1).len(), 1);
    }

    #[test]
    fn usage_report_parses_from_json() {
        let v = serde_json::json!({"agent":"codex","accountId":"a","windows":[{"key":"weekly","used":100,"resetsAt":123}]});
        let r: UsageReport = serde_json::from_value(v).unwrap();
        assert_eq!(r.windows[0].resets_at, Some(123));
        assert_eq!(r.windows[0].used, 100.0);
    }

    #[test]
    fn load_missing_file_is_empty() {
        let dir = std::env::temp_dir().join("panorama-cand-missing-nonexistent");
        assert!(load(&dir).entries(NOW).is_empty());
    }
}
