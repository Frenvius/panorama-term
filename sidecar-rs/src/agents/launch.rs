use std::path::Path;

use crate::agents::model::Agent;
use crate::orch::model::Effort;
use crate::orch::profiles::{resume_style, Profile};
use crate::orch::worker::{launch_command, valid_session_id, LaunchCommand};

pub fn resumable(exe: &str) -> bool {
    resume_style(exe).is_some()
}

pub fn session_cli(binding_agent: &str) -> &str {
    if binding_agent == "antigravity" { "agy" } else { binding_agent }
}

pub fn has_status_hook(exe: &str) -> bool {
    matches!(exe, "claude" | "codex" | "pi" | "agy")
}

pub fn resume_id_for<'a, F>(agent: &'a Agent, profile: &Profile, transcript_exists: F) -> Option<&'a str>
where
    F: Fn(&str, &str, &str) -> bool,
{
    let session = agent.native_session.as_ref()?;
    if session.agent != profile.exe || !resumable(&profile.exe) || !valid_session_id(&session.id) {
        return None;
    }
    if session.model != agent.model {
        return None;
    }
    if matches!(profile.exe.as_str(), "claude" | "codex") && !transcript_exists(&profile.exe, &session.id, &agent.cwd) {
        return None;
    }
    Some(&session.id)
}

pub fn native_transcript_exists(home: Option<&Path>) -> impl Fn(&str, &str, &str) -> bool + '_ {
    move |exe, id, cwd| home.is_some_and(|home| crate::native_worker_transcript(home, exe, id, cwd).is_some())
}

pub struct AgentCommand<'a> {
    pub agent: &'a Agent,
    pub profile: &'a Profile,
    pub cli: &'a str,
    pub pinned_env: &'a [(String, String)],
    pub settings_path: Option<&'a str>,
    pub resume_id: Option<&'a str>,
}

pub fn agent_command(input: AgentCommand<'_>) -> String {
    let AgentCommand { agent, profile, cli, pinned_env, settings_path, resume_id } = input;
    launch_command(LaunchCommand {
        profile,
        model: agent.model.as_deref(),
        effort: agent.effort.as_deref().and_then(Effort::from_str_opt),
        settings_path,
        resume_id,
        pinned_env,
        cli,
        prompt: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::model::NativeSession;
    use crate::agents::reducer::tests::agent;

    fn profiles() -> Vec<Profile> {
        let dir = std::env::temp_dir().join(format!("agents-launch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut profiles = crate::orch::profiles::load_profiles(&dir);
        for p in profiles.iter_mut() {
            p.probe_permissions = false;
        }
        profiles
    }

    fn command(profile: &Profile, resume_id: Option<&str>) -> String {
        let a = agent("a1", "rev");
        let a = Agent { model: None, ..a };
        agent_command(AgentCommand { agent: &a, profile, cli: "/cli", pinned_env: &[], settings_path: None, resume_id })
    }

    #[test]
    fn fresh_and_resume_command_per_cli() {
        let expected = [
            ("claude", " --resume sid"),
            ("codex", " sid"),
            ("opencode", " -s sid"),
            ("kimi", " -S sid"),
            ("antigravity", " --conversation sid"),
            ("pi", " --session sid"),
        ];
        let profiles = profiles();
        for (id, suffix) in expected {
            let profile = profiles.iter().find(|p| p.id == id).unwrap();
            let fresh = command(profile, None);
            assert!(fresh.starts_with(&profile.exe), "{fresh}");
            assert!(!fresh.contains("sid") && !fresh.contains("Read the file"), "{fresh}");
            let resumed = command(profile, Some("sid"));
            assert!(resumed.ends_with(suffix), "{id}: {resumed}");
        }
        let codex = profiles.iter().find(|p| p.id == "codex").unwrap();
        assert!(command(codex, Some("sid")).starts_with("codex resume "));
    }

    #[test]
    fn claude_agent_carries_settings_and_model_alias() {
        let profiles = profiles();
        let claude = profiles.iter().find(|p| p.id == "claude").unwrap();
        let a = agent("a1", "rev");
        let cmd = agent_command(AgentCommand { agent: &a, profile: claude, cli: "/cli", pinned_env: &[], settings_path: Some("/s.json"), resume_id: None });
        assert!(cmd.contains("--strict-mcp-config"), "{cmd}");
        assert!(cmd.contains("--settings /s.json"), "{cmd}");
        assert!(cmd.contains("--model claude-haiku-5-5"), "{cmd}");
    }

    #[test]
    fn resume_requires_same_cli_model_and_transcript() {
        let profiles = profiles();
        let claude = profiles.iter().find(|p| p.id == "claude").unwrap();
        let kimi = profiles.iter().find(|p| p.id == "kimi").unwrap();
        let mut a = agent("a1", "rev");
        let yes = |_: &str, _: &str, _: &str| true;
        let no = |_: &str, _: &str, _: &str| false;
        assert_eq!(resume_id_for(&a, claude, yes), None);
        a.native_session = Some(NativeSession { agent: "claude".into(), id: "sid-1".into(), bound_at: 1, model: a.model.clone() });
        assert_eq!(resume_id_for(&a, claude, yes), Some("sid-1"));
        assert_eq!(resume_id_for(&a, claude, no), None);
        assert_eq!(resume_id_for(&a, kimi, yes), None);
        a.model = Some("sonnet".into());
        assert_eq!(resume_id_for(&a, claude, yes), None);
        a.native_session = Some(NativeSession { agent: "kimi".into(), id: "session_x".into(), bound_at: 1, model: a.model.clone() });
        assert_eq!(resume_id_for(&a, kimi, no), Some("session_x"));
        a.native_session.as_mut().unwrap().id = "bad id".into();
        assert_eq!(resume_id_for(&a, kimi, yes), None);
    }

    #[test]
    fn unknown_cli_is_not_resumable() {
        assert!(!resumable("aider"));
        assert_eq!(session_cli("antigravity"), "agy");
        assert_eq!(session_cli("claude"), "claude");
    }
}
