use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{json, Value};

use crate::agents::model::*;

pub const RESTART_WINDOW_MS: u64 = 10 * 60 * 1000;
pub const RESTART_MAX: usize = 3;

const EXIT_DETAIL_MAX: usize = 400;
const EXIT_REASONS: &[&str] = &["session_gone", "shell_returned", "resume_failed"];
const CLEAR_REASONS: &[&str] = &["resume_failed", "cwd_changed", "profile_changed", "user"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentError {
    pub code: &'static str,
    pub message: String,
}

impl AgentError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

fn bad(message: impl Into<String>) -> AgentError { AgentError::new("bad_request", message) }

#[derive(Debug)]
pub enum ApplyResult {
    Ok(String, Value),
    Unchanged,
    Err(AgentError),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AgentPatch {
    pub name: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub profile: Option<String>,
    pub model: Option<Option<String>>,
    pub effort: Option<Option<String>>,
    pub cwd: Option<String>,
    pub project: Option<Option<String>>,
    pub instructions: Option<String>,
    pub auto_start: Option<bool>,
}

impl AgentPatch {
    fn to_json(&self) -> Value {
        let mut obj = serde_json::Map::new();
        let mut put = |key: &str, value: Option<Value>| {
            if let Some(v) = value { obj.insert(key.into(), v); }
        };
        put("name", self.name.clone().map(Value::from));
        put("title", self.title.clone().map(Value::from));
        put("description", self.description.clone().map(Value::from));
        put("profile", self.profile.clone().map(Value::from));
        put("model", self.model.clone().map(|m| json!(m)));
        put("effort", self.effort.clone().map(|e| json!(e)));
        put("cwd", self.cwd.clone().map(Value::from));
        put("project", self.project.clone().map(|p| json!(p)));
        put("instructions", self.instructions.clone().map(Value::from));
        put("autoStart", self.auto_start.map(Value::from));
        Value::Object(obj)
    }
}

pub enum Cmd {
    Create { agent: Agent, known_profiles: Vec<String> },
    Update { id: String, patch: AgentPatch, known_profiles: Vec<String>, at: u64 },
    Start { id: String, at: u64, live: bool, resume_id: Option<String> },
    Stop { id: String, at: u64, by: String, force: bool, crashed: bool },
    Exit { id: String, at: u64, reason: String, code: Option<i32>, detail: Option<String> },
    Restart { id: String, at: u64 },
    BindSession { id: String, agent: String, session_id: String, at: u64 },
    SessionClear { id: String, at: u64, reason: String },
}

#[derive(Debug, Default)]
pub struct State {
    pub agents: BTreeMap<String, Agent>,
    pub seqs: BTreeMap<String, u32>,
}

pub fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= NAME_MAX
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn valid_effort(effort: &str) -> bool {
    crate::orch::model::Effort::from_str_opt(effort).is_some()
}

fn check_fields(agent: &Agent, known_profiles: &[String]) -> Result<(), AgentError> {
    if !valid_name(&agent.name) {
        return Err(bad("name must match [a-z0-9][a-z0-9-]{0,31}"));
    }
    if agent.title.chars().count() > TITLE_MAX_CHARS {
        return Err(bad(format!("title is longer than {TITLE_MAX_CHARS} characters")));
    }
    if agent.description.chars().count() > DESCRIPTION_MAX_CHARS {
        return Err(bad(format!("description is longer than {DESCRIPTION_MAX_CHARS} characters")));
    }
    if agent.instructions.len() > INSTRUCTIONS_MAX_BYTES {
        return Err(bad(format!("instructions are longer than {INSTRUCTIONS_MAX_BYTES} bytes")));
    }
    if !known_profiles.iter().any(|p| *p == agent.profile) {
        return Err(bad(format!("unknown profile: {}", agent.profile)));
    }
    if agent.effort.as_deref().is_some_and(|e| !valid_effort(e)) {
        return Err(bad("effort must be low, medium, high, xhigh or max"));
    }
    let cwd = Path::new(&agent.cwd);
    if !cwd.is_absolute() || !cwd.is_dir() {
        return Err(bad("cwd must be an existing absolute directory"));
    }
    Ok(())
}

fn prune_restarts(restarts: &[u64], at: u64) -> Vec<u64> {
    restarts.iter().copied().filter(|t| at.saturating_sub(*t) < RESTART_WINDOW_MS).collect()
}

impl State {
    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.agents.values().any(|a| Some(a.id.as_str()) != except && a.name.eq_ignore_ascii_case(name))
    }

    pub fn resolve(&self, key: &str) -> Option<&Agent> {
        self.agents.get(key).or_else(|| self.agents.values().find(|a| a.name.eq_ignore_ascii_case(key)))
    }

    fn get(&self, id: &str) -> Result<&Agent, AgentError> {
        self.agents.get(id).ok_or_else(|| AgentError::new("not_found", "agent not found"))
    }

    pub fn apply(&self, cmd: &Cmd) -> ApplyResult {
        match self.try_apply(cmd) {
            Ok(Some((kind, data))) => ApplyResult::Ok(kind.into(), data),
            Ok(None) => ApplyResult::Unchanged,
            Err(e) => ApplyResult::Err(e),
        }
    }

    fn try_apply(&self, cmd: &Cmd) -> Result<Option<(&'static str, Value)>, AgentError> {
        match cmd {
            Cmd::Create { agent, known_profiles } => {
                if self.agents.contains_key(&agent.id) {
                    return Err(AgentError::new("conflict", "agent id exists"));
                }
                check_fields(agent, known_profiles)?;
                if self.name_taken(&agent.name, None) {
                    return Err(AgentError::new("conflict", format!("name {} is taken", agent.name)));
                }
                let data = serde_json::to_value(agent).map_err(|e| bad(e.to_string()))?;
                Ok(Some(("agent-create", data)))
            }
            Cmd::Update { id, patch, known_profiles, at } => {
                let current = self.get(id)?;
                let mut next = current.clone();
                apply_patch(&mut next, patch);
                check_fields(&next, known_profiles)?;
                if next.name != current.name && self.name_taken(&next.name, Some(id)) {
                    return Err(AgentError::new("conflict", format!("name {} is taken", next.name)));
                }
                if next == *current {
                    return Ok(None);
                }
                let clear_session = current.native_session.is_some()
                    && (next.cwd != current.cwd || next.profile != current.profile);
                Ok(Some(("agent-update", json!({
                    "id": id, "at": at, "fields": patch.to_json(), "clearSession": clear_session,
                }))))
            }
            Cmd::Start { id, at, live, resume_id } => {
                let agent = self.get(id)?;
                if agent.desired == Desired::Running && *live {
                    return Ok(None);
                }
                Ok(Some(("agent-start", json!({ "id": id, "at": at, "resumeId": resume_id }))))
            }
            Cmd::Stop { id, at, by, force, crashed } => {
                let agent = self.get(id)?;
                if agent.desired == Desired::Stopped && agent.crashed == *crashed {
                    return Ok(None);
                }
                if !matches!(by.as_str(), "user" | "brain") {
                    return Err(bad("by must be user or brain"));
                }
                Ok(Some(("agent-stop", json!({ "id": id, "at": at, "by": by, "force": force, "crashed": crashed }))))
            }
            Cmd::Exit { id, at, reason, code, detail } => {
                self.get(id)?;
                if !EXIT_REASONS.contains(&reason.as_str()) {
                    return Err(bad(format!("unknown exit reason {reason}")));
                }
                let detail: Option<String> = detail.as_deref().map(|d| d.chars().take(EXIT_DETAIL_MAX).collect());
                Ok(Some(("agent-exit", json!({ "id": id, "at": at, "reason": reason, "code": code, "detail": detail }))))
            }
            Cmd::Restart { id, at } => {
                let agent = self.get(id)?;
                if prune_restarts(&agent.restarts, *at).len() >= RESTART_MAX {
                    return Err(AgentError::new("restart_limit", format!("{RESTART_MAX} restarts in 10 minutes")));
                }
                Ok(Some(("agent-restart", json!({ "id": id, "at": at }))))
            }
            Cmd::BindSession { id, agent: cli, session_id, at } => {
                let agent = self.get(id)?;
                if session_id.is_empty() {
                    return Err(bad("sessionId required"));
                }
                if agent.native_session.as_ref().is_some_and(|s| s.agent == *cli && s.id == *session_id) {
                    return Ok(None);
                }
                Ok(Some(("agent-bind-session", json!({ "id": id, "agent": cli, "sessionId": session_id, "at": at }))))
            }
            Cmd::SessionClear { id, at, reason } => {
                let agent = self.get(id)?;
                if !CLEAR_REASONS.contains(&reason.as_str()) {
                    return Err(bad(format!("unknown clear reason {reason}")));
                }
                if agent.native_session.is_none() {
                    return Ok(None);
                }
                Ok(Some(("agent-session-clear", json!({ "id": id, "at": at, "reason": reason }))))
            }
        }
    }

    pub fn replay(&mut self, kind: &str, data: &Value) -> Result<(), String> {
        if kind == "agent-create" {
            let agent: Agent = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
            self.agents.insert(agent.id.clone(), agent);
            return Ok(());
        }
        let id = data["id"].as_str().ok_or("missing id")?;
        let at = data["at"].as_u64().unwrap_or(0);
        let Some(agent) = self.agents.get_mut(id) else {
            return Err(format!("unknown agent {id}"));
        };
        match kind {
            "agent-update" => {
                let patch = patch_from_json(&data["fields"]);
                apply_patch(agent, &patch);
                if data["clearSession"].as_bool() == Some(true) {
                    agent.native_session = None;
                }
            }
            "agent-start" => {
                agent.desired = Desired::Running;
                agent.crashed = false;
                agent.restarts.clear();
                agent.last_started_at = Some(at);
            }
            "agent-stop" => {
                agent.desired = Desired::Stopped;
                agent.crashed = data["crashed"].as_bool().unwrap_or(false);
            }
            "agent-exit" => {
                agent.last_exit = Some(Exit {
                    at,
                    reason: data["reason"].as_str().unwrap_or_default().into(),
                    code: data["code"].as_i64().map(|c| c as i32),
                    detail: data["detail"].as_str().map(str::to_string),
                });
            }
            "agent-restart" => {
                let mut restarts = prune_restarts(&agent.restarts, at);
                restarts.push(at);
                agent.restarts = restarts;
                agent.last_started_at = Some(at);
            }
            "agent-bind-session" => {
                agent.native_session = Some(NativeSession {
                    agent: data["agent"].as_str().unwrap_or_default().into(),
                    id: data["sessionId"].as_str().unwrap_or_default().into(),
                    bound_at: at,
                    model: agent.model.clone(),
                });
            }
            "agent-session-clear" => agent.native_session = None,
            _ => return Ok(()),
        }
        agent.updated_at = agent.updated_at.max(at);
        Ok(())
    }
}

fn opt_string(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

pub fn patch_from_json(fields: &Value) -> AgentPatch {
    let text = |key: &str| fields.get(key).and_then(Value::as_str).map(str::to_string);
    let nullable = |key: &str| fields.get(key).map(opt_string);
    AgentPatch {
        name: text("name"),
        title: text("title"),
        description: text("description"),
        profile: text("profile"),
        model: nullable("model"),
        effort: nullable("effort"),
        cwd: text("cwd"),
        project: nullable("project"),
        instructions: text("instructions"),
        auto_start: fields.get("autoStart").and_then(Value::as_bool),
    }
}

fn apply_patch(agent: &mut Agent, patch: &AgentPatch) {
    if let Some(v) = &patch.name { agent.name = v.clone(); }
    if let Some(v) = &patch.title { agent.title = v.clone(); }
    if let Some(v) = &patch.description { agent.description = v.clone(); }
    if let Some(v) = &patch.profile { agent.profile = v.clone(); }
    if let Some(v) = &patch.model { agent.model = v.clone(); }
    if let Some(v) = &patch.effort { agent.effort = v.clone(); }
    if let Some(v) = &patch.cwd { agent.cwd = v.clone(); }
    if let Some(v) = &patch.project { agent.project = v.clone(); }
    if let Some(v) = &patch.instructions { agent.instructions = v.clone(); }
    if let Some(v) = patch.auto_start { agent.auto_start = v; }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn tmp_cwd() -> String {
        std::env::temp_dir().to_string_lossy().replace('\\', "/")
    }

    pub(crate) fn agent(id: &str, name: &str) -> Agent {
        Agent {
            id: id.into(), name: name.into(), title: "Reviewer".into(), description: String::new(),
            profile: "claude".into(), model: Some("haiku".into()), effort: None, cwd: tmp_cwd(), project: None,
            instructions: "Answer in one line.".into(), auto_start: false, desired: Desired::Stopped,
            native_session: None, last_exit: None, restarts: vec![], crashed: false,
            created_at: 1, updated_at: 1, last_started_at: None,
        }
    }

    fn profiles() -> Vec<String> { vec!["claude".into(), "codex".into()] }

    fn commit(state: &mut State, cmd: Cmd) -> Result<Option<(String, Value)>, AgentError> {
        match state.apply(&cmd) {
            ApplyResult::Ok(kind, data) => {
                state.replay(&kind, &data).unwrap();
                Ok(Some((kind, data)))
            }
            ApplyResult::Unchanged => Ok(None),
            ApplyResult::Err(e) => Err(e),
        }
    }

    fn create(state: &mut State, id: &str, name: &str) {
        commit(state, Cmd::Create { agent: agent(id, name), known_profiles: profiles() }).unwrap();
    }

    #[test]
    fn create_validates_name_profile_cwd_and_caps() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        let cases: Vec<(Agent, &str)> = vec![
            (Agent { name: "Rev".into(), ..agent("a2", "") }, "bad_request"),
            (Agent { name: "-rev".into(), ..agent("a2", "") }, "bad_request"),
            (agent("a2", &"x".repeat(33)), "bad_request"),
            (agent("a2", "rev"), "conflict"),
            (Agent { profile: "nope".into(), ..agent("a2", "other") }, "bad_request"),
            (Agent { cwd: "/definitely/not/here/xyz".into(), ..agent("a2", "other") }, "bad_request"),
            (Agent { cwd: "relative/dir".into(), ..agent("a2", "other") }, "bad_request"),
            (Agent { instructions: "x".repeat(INSTRUCTIONS_MAX_BYTES + 1), ..agent("a2", "other") }, "bad_request"),
            (Agent { title: "t".repeat(TITLE_MAX_CHARS + 1), ..agent("a2", "other") }, "bad_request"),
            (Agent { effort: Some("huge".into()), ..agent("a2", "other") }, "bad_request"),
        ];
        for (candidate, code) in cases {
            let name = candidate.name.clone();
            let err = commit(&mut state, Cmd::Create { agent: candidate, known_profiles: profiles() }).unwrap_err();
            assert_eq!(err.code, code, "{name}: {}", err.message);
        }
        let ok = Agent { instructions: "x".repeat(INSTRUCTIONS_MAX_BYTES), ..agent("a3", "other-1") };
        commit(&mut state, Cmd::Create { agent: ok, known_profiles: profiles() }).unwrap();
        assert_eq!(state.agents.len(), 2);
    }

    #[test]
    fn duplicate_name_check_ignores_case() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        state.agents.get_mut("a1").unwrap().name = "REV".into();
        let err = commit(&mut state, Cmd::Create { agent: agent("a2", "rev"), known_profiles: profiles() }).unwrap_err();
        assert_eq!(err.code, "conflict");
    }

    #[test]
    fn update_is_partial_and_rename_conflicts() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        create(&mut state, "a2", "impl");
        let patch = AgentPatch { title: Some("Senior reviewer".into()), ..Default::default() };
        commit(&mut state, Cmd::Update { id: "a1".into(), patch, known_profiles: profiles(), at: 5 }).unwrap();
        let a1 = &state.agents["a1"];
        assert_eq!(a1.title, "Senior reviewer");
        assert_eq!(a1.model.as_deref(), Some("haiku"));
        assert_eq!(a1.updated_at, 5);

        let rename = AgentPatch { name: Some("impl".into()), ..Default::default() };
        let err = commit(&mut state, Cmd::Update { id: "a1".into(), patch: rename, known_profiles: profiles(), at: 6 }).unwrap_err();
        assert_eq!(err.code, "conflict");

        let clear_model = AgentPatch { model: Some(None), ..Default::default() };
        commit(&mut state, Cmd::Update { id: "a1".into(), patch: clear_model, known_profiles: profiles(), at: 7 }).unwrap();
        assert_eq!(state.agents["a1"].model, None);

        let same = AgentPatch { title: Some("Senior reviewer".into()), ..Default::default() };
        assert!(commit(&mut state, Cmd::Update { id: "a1".into(), patch: same, known_profiles: profiles(), at: 8 }).unwrap().is_none());
    }

    #[test]
    fn cwd_or_profile_change_clears_the_session_in_the_same_event() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        for (patch, expect_clear) in [
            (AgentPatch { title: Some("x".into()), ..Default::default() }, false),
            (AgentPatch { profile: Some("codex".into()), ..Default::default() }, true),
            (AgentPatch { cwd: Some(std::env::temp_dir().join("..").to_string_lossy().into_owned()), ..Default::default() }, true),
        ] {
            commit(&mut state, Cmd::BindSession { id: "a1".into(), agent: "claude".into(), session_id: format!("s{expect_clear}"), at: 2 }).unwrap();
            let (_, data) = commit(&mut state, Cmd::Update { id: "a1".into(), patch, known_profiles: profiles(), at: 3 }).unwrap().unwrap();
            assert_eq!(data["clearSession"], expect_clear);
            assert_eq!(state.agents["a1"].native_session.is_none(), expect_clear);
        }
    }

    #[test]
    fn start_is_idempotent_only_while_live() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        assert!(commit(&mut state, Cmd::Start { id: "a1".into(), at: 10, live: false, resume_id: None }).unwrap().is_some());
        assert_eq!(state.agents["a1"].desired, Desired::Running);
        assert_eq!(state.agents["a1"].last_started_at, Some(10));
        assert!(commit(&mut state, Cmd::Start { id: "a1".into(), at: 11, live: true, resume_id: None }).unwrap().is_none());
        assert!(commit(&mut state, Cmd::Start { id: "a1".into(), at: 12, live: false, resume_id: None }).unwrap().is_some());
    }

    #[test]
    fn stop_exit_and_crash_marker() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        commit(&mut state, Cmd::Start { id: "a1".into(), at: 1, live: false, resume_id: None }).unwrap();
        commit(&mut state, Cmd::Exit { id: "a1".into(), at: 2, reason: "shell_returned".into(), code: Some(1), detail: Some("x".repeat(500)) }).unwrap();
        assert_eq!(state.agents["a1"].last_exit, Some(Exit { at: 2, reason: "shell_returned".into(), code: Some(1), detail: Some("x".repeat(EXIT_DETAIL_MAX)) }));
        assert!(commit(&mut state, Cmd::Exit { id: "a1".into(), at: 2, reason: "bogus".into(), code: None, detail: None }).is_err());
        commit(&mut state, Cmd::Stop { id: "a1".into(), at: 3, by: "brain".into(), force: true, crashed: true }).unwrap();
        assert!(state.agents["a1"].crashed);
        assert_eq!(state.agents["a1"].desired, Desired::Stopped);
        assert!(commit(&mut state, Cmd::Stop { id: "a1".into(), at: 4, by: "brain".into(), force: true, crashed: true }).unwrap().is_none());
        commit(&mut state, Cmd::Start { id: "a1".into(), at: 5, live: false, resume_id: None }).unwrap();
        assert!(!state.agents["a1"].crashed);
        commit(&mut state, Cmd::Stop { id: "a1".into(), at: 6, by: "user".into(), force: false, crashed: false }).unwrap();
        assert!(commit(&mut state, Cmd::Stop { id: "a1".into(), at: 7, by: "user".into(), force: false, crashed: false }).unwrap().is_none());
    }

    #[test]
    fn fourth_restart_in_ten_minutes_is_rejected() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        let base = 1_000_000_000u64;
        for i in 0..3 {
            commit(&mut state, Cmd::Restart { id: "a1".into(), at: base + i * 60_000 }).unwrap();
        }
        let err = commit(&mut state, Cmd::Restart { id: "a1".into(), at: base + 4 * 60_000 }).unwrap_err();
        assert_eq!(err.code, "restart_limit");
        commit(&mut state, Cmd::Restart { id: "a1".into(), at: base + RESTART_WINDOW_MS + 1 }).unwrap();
        assert_eq!(state.agents["a1"].restarts.len(), 3);
        commit(&mut state, Cmd::Start { id: "a1".into(), at: base + RESTART_WINDOW_MS + 2, live: false, resume_id: None }).unwrap();
        assert!(state.agents["a1"].restarts.is_empty());
    }

    #[test]
    fn bind_session_dedupes_and_clear_removes() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        let bind = || Cmd::BindSession { id: "a1".into(), agent: "claude".into(), session_id: "s1".into(), at: 2 };
        assert!(commit(&mut state, bind()).unwrap().is_some());
        assert!(commit(&mut state, bind()).unwrap().is_none());
        assert_eq!(state.agents["a1"].native_session.as_ref().unwrap().bound_at, 2);
        assert!(commit(&mut state, Cmd::SessionClear { id: "a1".into(), at: 3, reason: "nope".into() }).is_err());
        commit(&mut state, Cmd::SessionClear { id: "a1".into(), at: 3, reason: "user".into() }).unwrap();
        assert!(state.agents["a1"].native_session.is_none());
        assert!(commit(&mut state, Cmd::SessionClear { id: "a1".into(), at: 4, reason: "user".into() }).unwrap().is_none());
    }

    #[test]
    fn replay_of_applied_events_matches_live_state() {
        let mut live = State::default();
        let mut log = Vec::new();
        let mut record = |state: &mut State, cmd: Cmd| {
            if let Some(event) = commit(state, cmd).unwrap() { log.push(event); }
        };
        record(&mut live, Cmd::Create { agent: agent("a1", "rev"), known_profiles: profiles() });
        record(&mut live, Cmd::Start { id: "a1".into(), at: 2, live: false, resume_id: None });
        record(&mut live, Cmd::BindSession { id: "a1".into(), agent: "claude".into(), session_id: "s1".into(), at: 3 });
        record(&mut live, Cmd::Exit { id: "a1".into(), at: 4, reason: "session_gone".into(), code: None, detail: None });
        record(&mut live, Cmd::Restart { id: "a1".into(), at: 5 });
        record(&mut live, Cmd::Update { id: "a1".into(), patch: AgentPatch { instructions: Some("new".into()), ..Default::default() }, known_profiles: profiles(), at: 6 });
        let mut replayed = State::default();
        for (kind, data) in &log {
            replayed.replay(kind, data).unwrap();
        }
        assert_eq!(replayed.agents, live.agents);
    }

    #[test]
    fn unknown_kind_is_skipped_and_missing_fields_default() {
        let mut state = State::default();
        state.replay("agent-create", &json!({"id":"a1","name":"rev","profile":"claude","cwd":"/tmp"})).unwrap();
        state.replay("agent-future-thing", &json!({"id":"a1"})).unwrap();
        let a = &state.agents["a1"];
        assert!(a.auto_start);
        assert_eq!(a.desired, Desired::Stopped);
        assert!(a.restarts.is_empty() && a.native_session.is_none() && !a.crashed);
    }

    #[test]
    fn resolve_by_id_then_name() {
        let mut state = State::default();
        create(&mut state, "a1", "rev");
        assert_eq!(state.resolve("a1").unwrap().name, "rev");
        assert_eq!(state.resolve("REV").unwrap().id, "a1");
        assert!(state.resolve("nope").is_none());
    }
}
