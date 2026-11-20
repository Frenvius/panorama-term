use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};

fn is_false(b: &bool) -> bool { !b }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_flag: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub ready_signal: ReadySignal,
    #[serde(default)]
    pub startup_answers: Vec<StartupAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_text: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub provider: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReadySignal {
    Status,
    #[default]
    Quiet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupAnswer {
    pub contains: String,
    pub send: String,
}

fn cli_profile(id: &str, args: &[&str], model_flag: &str) -> Profile {
    Profile {
        id: id.into(),
        exe: id.into(),
        args: args.iter().map(|a| a.to_string()).collect(),
        model_flag: Some(model_flag.into()),
        env: HashMap::new(),
        notes: None,
        ready_signal: ReadySignal::Quiet,
        startup_answers: Vec::new(),
        ready_text: None,
        provider: false,
    }
}

fn built_ins() -> Vec<Profile> {
    let claude = Profile {
        ready_signal: ReadySignal::Status,
        startup_answers: vec![
            StartupAnswer { contains: "Yes, I trust this folder".into(), send: "\x1b[B\r".into() },
            StartupAnswer { contains: "Continue without using this MCP server".into(), send: "\r".into() },
        ],
        ..cli_profile(
            "claude",
            &["--permission-mode", "acceptEdits", "--allowedTools", "Bash({cli} orch:*)", "PowerShell({cli} orch:*)"],
            "--model",
        )
    };
    let opencode = Profile {
        notes: Some("model in provider/model format".into()),
        ready_text: Some("ctrl+p commands".into()),
        startup_answers: vec![StartupAnswer {
            contains: "Please restart the application".into(),
            send: "\x1b".into(),
        }],
        ..cli_profile("opencode", &[], "--model")
    };
    vec![
        claude,
        cli_profile("codex", &["--full-auto"], "-m"),
        opencode,
        Profile {
            ready_text: Some("context:".into()),
            startup_answers: vec![StartupAnswer { contains: "Trust this folder?".into(), send: "\r".into() }],
            ..cli_profile("kimi", &["-y"], "-m")
        },
        cli_profile("pi", &[], "--model"),
    ]
}

pub fn load_profiles(base: &Path) -> Vec<Profile> {
    let path = base.join("orch").join("profiles.json");
    let overrides: Vec<Profile> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let mut map: HashMap<String, Profile> = built_ins()
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();

    let settings = crate::orch::settings::load(base);
    for provider in &settings.providers {
        let profile = provider_to_profile(provider);
        map.insert(profile.id.clone(), profile);
    }

    for p in overrides {
        map.insert(p.id.clone(), p);
    }

    let mut result: Vec<Profile> = map.into_values().collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

fn provider_to_profile(provider: &crate::orch::settings::OrchProvider) -> Profile {
    let claude_base = built_ins().into_iter().find(|p| p.id == "claude")
        .expect("built-in claude profile must exist");

    let mut env = HashMap::new();
    env.insert("ANTHROPIC_BASE_URL".into(), format!("literal:{}", provider.base_url));
    env.insert("ANTHROPIC_AUTH_TOKEN".into(), format!("secret:{}", provider.id));
    env.insert("ANTHROPIC_API_KEY".into(), "literal:".into());
    env.insert("ANTHROPIC_MODEL".into(), format!("literal:{}", provider.main_model));
    env.insert("ANTHROPIC_DEFAULT_OPUS_MODEL".into(), format!("literal:{}", provider.main_model));
    env.insert("ANTHROPIC_DEFAULT_SONNET_MODEL".into(), format!("literal:{}", provider.main_model));
    env.insert("ANTHROPIC_DEFAULT_HAIKU_MODEL".into(), format!("literal:{}", provider.fast_model));
    env.insert("CLAUDE_CODE_SUBAGENT_MODEL".into(), format!("literal:{}", provider.fast_model));
    env.insert("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".into(), "literal:1".into());

    for preset in crate::orch::settings::presets() {
        if preset.id == provider.preset {
            for (k, v) in preset.extra_env {
                env.insert(k.to_string(), format!("literal:{v}"));
            }
            break;
        }
    }

    Profile {
        id: provider.id.clone(),
        exe: "claude".into(),
        args: claude_base.args.clone(),
        model_flag: Some("--model".into()),
        env,
        notes: None,
        ready_signal: ReadySignal::Status,
        startup_answers: claude_base.startup_answers.clone(),
        ready_text: None,
        provider: true,
    }
}

pub fn find_profile<'a>(profiles: &'a [Profile], id: &str) -> Option<&'a Profile> {
    profiles.iter().find(|p| p.id == id)
}

pub fn resolve_env(env: &HashMap<String, String>, base: Option<&Path>) -> Vec<(String, String)> {
    env.iter()
        .filter_map(|(k, v)| {
            let val = if let Some(lit) = v.strip_prefix("literal:") {
                lit.to_string()
            } else if let Some(id) = v.strip_prefix("secret:") {
                crate::orch::secrets::get(id, base)?
            } else {
                std::env::var(v).ok()?
            };
            Some((k.clone(), val))
        })
        .collect()
}

pub fn build_command_line(profile: &Profile, model: Option<&str>) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    tokens.push(profile.exe.clone());
    for arg in &profile.args {
        tokens.push(arg.clone());
    }
    if let (Some(flag), Some(model_val)) = (&profile.model_flag, model) {
        tokens.push(flag.clone());
        tokens.push(model_val.to_string());
    }
    tokens
}

pub fn tokens_to_command(tokens: &[String]) -> String {
    tokens.iter().map(|t| {
        if t.contains(' ') {
            format!("\"{t}\"")
        } else {
            t.clone()
        }
    }).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp_dir(suffix: &str) -> PathBuf {
        let ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("orch-profiles-{suffix}-{ns}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn built_ins_have_expected_flags() {
        let profiles = built_ins();
        let claude = profiles.iter().find(|p| p.id == "claude").unwrap();
        assert!(claude.args.contains(&"--permission-mode".to_string()));
        assert!(claude.args.contains(&"acceptEdits".to_string()));
        assert_eq!(claude.model_flag.as_deref(), Some("--model"));

        let codex = profiles.iter().find(|p| p.id == "codex").unwrap();
        assert!(codex.args.contains(&"--full-auto".to_string()));
        assert_eq!(codex.model_flag.as_deref(), Some("-m"));

        let opencode = profiles.iter().find(|p| p.id == "opencode").unwrap();
        assert_eq!(opencode.model_flag.as_deref(), Some("--model"));

        let kimi = profiles.iter().find(|p| p.id == "kimi").unwrap();
        assert_eq!(kimi.model_flag.as_deref(), Some("-m"));

        let pi = profiles.iter().find(|p| p.id == "pi").unwrap();
        assert_eq!(pi.model_flag.as_deref(), Some("--model"));
    }

    #[test]
    fn user_profile_overrides_built_in() {
        let dir = tmp_dir("override");
        let orch_dir = dir.join("orch");
        std::fs::create_dir_all(&orch_dir).unwrap();
        let custom = serde_json::json!([{
            "id": "claude",
            "exe": "claude-custom",
            "args": ["--dangerously-skip-permissions"],
            "modelFlag": "--model"
        }]);
        std::fs::write(dir.join("orch").join("profiles.json"), custom.to_string()).unwrap();
        let profiles = load_profiles(&dir);
        let claude = profiles.iter().find(|p| p.id == "claude").unwrap();
        assert_eq!(claude.exe, "claude-custom");
        assert!(claude.args.contains(&"--dangerously-skip-permissions".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn user_profile_extends_built_ins() {
        let dir = tmp_dir("extend");
        let orch_dir = dir.join("orch");
        std::fs::create_dir_all(&orch_dir).unwrap();
        let custom = serde_json::json!([{
            "id": "deepseek",
            "exe": "opencode",
            "modelFlag": "--model",
            "args": []
        }]);
        std::fs::write(dir.join("orch").join("profiles.json"), custom.to_string()).unwrap();
        let profiles = load_profiles(&dir);
        assert!(profiles.iter().any(|p| p.id == "deepseek"));
        assert!(profiles.iter().any(|p| p.id == "claude"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn env_resolution_literal_prefix() {
        let mut env = HashMap::new();
        env.insert("MY_VAR".to_string(), "literal:hello world".to_string());
        let resolved = resolve_env(&env, None);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0], ("MY_VAR".to_string(), "hello world".to_string()));
    }

    #[test]
    fn env_resolution_missing_var_skipped() {
        let mut env = HashMap::new();
        env.insert("MISSING_VAR_XYZ123".to_string(), "MISSING_ENV_VAR_XYZ123".to_string());
        let resolved = resolve_env(&env, None);
        assert!(resolved.is_empty());
    }

    #[test]
    fn env_resolution_secret_missing_drops_var() {
        let dir = tmp_dir("secret-miss");
        let mut env = HashMap::new();
        env.insert("SOME_KEY".to_string(), "secret:nonexistent-provider-xyz".to_string());
        let resolved = resolve_env(&env, Some(&dir));
        assert!(resolved.is_empty(), "missing secret must drop the var");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provider_profile_env_uses_secret_prefix() {
        let provider = crate::orch::settings::OrchProvider {
            id: "myprovider".into(),
            preset: "deepseek".into(),
            label: "My Provider".into(),
            base_url: "https://example.com".into(),
            main_model: "fast-model".into(),
            fast_model: "fast-model".into(),
        };
        let profile = super::provider_to_profile(&provider);
        let token_val = profile.env.get("ANTHROPIC_AUTH_TOKEN").expect("ANTHROPIC_AUTH_TOKEN must be set");
        assert!(token_val.starts_with("secret:"), "token must use secret: prefix, got {token_val}");
        assert_eq!(token_val, "secret:myprovider");
        assert!(profile.provider);
    }

    #[test]
    fn env_resolution_does_not_leak_values_in_serialization() {
        let profiles = built_ins();
        let json = serde_json::to_string(&profiles).unwrap();
        assert!(!json.contains("sk-"), "no secrets in serialized output");
    }

    #[test]
    fn command_line_no_model() {
        let p = cli_profile("claude", &["--permission-mode", "acceptEdits"], "--model");
        let tokens = build_command_line(&p, None);
        assert_eq!(tokens, vec!["claude", "--permission-mode", "acceptEdits"]);
    }

    #[test]
    fn command_line_with_model() {
        let p = cli_profile("claude", &["--permission-mode", "acceptEdits"], "--model");
        let tokens = build_command_line(&p, Some("claude-haiku-4-5"));
        let cmd = tokens_to_command(&tokens);
        assert!(cmd.contains("--model"));
        assert!(cmd.contains("claude-haiku-4-5"));
    }

    #[test]
    fn command_line_quoting_spaces() {
        let tokens = vec!["exe".to_string(), "value with spaces".to_string()];
        let cmd = tokens_to_command(&tokens);
        assert_eq!(cmd, "exe \"value with spaces\"");
    }
}
