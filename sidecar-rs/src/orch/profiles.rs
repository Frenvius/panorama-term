use std::collections::HashMap;
use std::path::Path;
use serde::{Deserialize, Serialize};
use crate::orch::model::Effort;

fn is_false(b: &bool) -> bool { !b }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EffortArg {
    Flag(String),
    ConfigKey(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub exe: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort_arg: Option<EffortArg>,
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
    #[serde(default)]
    pub brief_delivery: BriefDelivery,
    #[serde(default, skip_serializing_if = "is_false")]
    pub probe_permissions: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub pin_env_via_config: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BriefDelivery {
    #[default]
    Paste,
    ArgvPointer,
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
        effort_arg: None,
        env: HashMap::new(),
        notes: None,
        ready_signal: ReadySignal::Quiet,
        startup_answers: Vec::new(),
        ready_text: None,
        provider: false,
        brief_delivery: BriefDelivery::Paste,
        probe_permissions: false,
        pin_env_via_config: false,
    }
}

fn codex_profile() -> Profile {
    Profile {
        effort_arg: Some(EffortArg::ConfigKey("model_reasoning_effort".into())),
        brief_delivery: BriefDelivery::ArgvPointer,
        probe_permissions: true,
        pin_env_via_config: true,
        startup_answers: vec![
            StartupAnswer { contains: "Hooks need review".into(), send: "2\r".into() },
            StartupAnswer { contains: "Trust this folder?".into(), send: "\r".into() },
            StartupAnswer { contains: "1. Trust and continue".into(), send: "\r".into() },
        ],
        ..cli_profile("codex", &["-c", "check_for_update_on_startup=false"], "-m")
    }
}

pub fn codex_permission_args(help: &str) -> Vec<String> {
    let args: &[&str] = if help.contains("--approve-for-me") {
        &["--approve-for-me"]
    } else {
        &["-s", "workspace-write", "-a", "on-request"]
    };
    args.iter().map(|a| a.to_string()).collect()
}

pub fn brief_pointer_prompt(brief_path: &Path) -> String {
    format!("Read the file {} and follow it exactly.", brief_path.display())
}

pub fn quote_arg(arg: &str, powershell: bool) -> String {
    if powershell {
        format!("'{}'", arg.replace('\'', "''"))
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

const PINNED_ENV_PREFIX: &str = "PANORAMA_";

pub fn pinned_env_args(env: &[(String, String)], powershell: bool) -> Vec<String> {
    env.iter()
        .filter(|(k, _)| k.starts_with(PINNED_ENV_PREFIX))
        .flat_map(|(k, v)| {
            let override_arg = format!("shell_environment_policy.set.{k}={}", toml_basic_string(v));
            ["-c".to_string(), quote_arg(&override_arg, powershell)]
        })
        .collect()
}

fn built_ins() -> Vec<Profile> {
    let claude = Profile {
        effort_arg: Some(EffortArg::Flag("--effort".into())),
        ready_signal: ReadySignal::Status,
        startup_answers: vec![
            StartupAnswer { contains: "Yes, I trust this folder".into(), send: "\x1b[B\r".into() },
            StartupAnswer { contains: "Continue without using this MCP server".into(), send: "\r".into() },
        ],
        ..cli_profile(
            "claude",
            &[
                "--permission-mode", "acceptEdits", "--strict-mcp-config",
                "--allowedTools", "Bash({cli} orch:*)", "PowerShell({cli} orch:*)",
            ],
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
        codex_profile(),
        opencode,
        Profile {
            ready_text: Some("context:".into()),
            startup_answers: vec![
                StartupAnswer { contains: "Trust this folder?".into(), send: "\r".into() },
                StartupAnswer { contains: "Continue with current version".into(), send: "\x1b".into() },
            ],
            ..cli_profile("kimi", &["-y"], "-m")
        },
        Profile {
            effort_arg: Some(EffortArg::Flag("--thinking".into())),
            startup_answers: vec![StartupAnswer { contains: "Do not trust (this session only)".into(), send: "\r".into() }],
            ..cli_profile("pi", &[], "--model")
        },
        {
            let mut env = std::collections::HashMap::new();
            env.insert("AGY_CLI_DISABLE_AUTO_UPDATE".into(), "literal:true".into());
            Profile {
                id: "antigravity".into(),
                exe: "agy".into(),
                args: vec!["--dangerously-skip-permissions".into()],
                model_flag: Some("--model".into()),
                effort_arg: Some(EffortArg::Flag("--effort".into())),
                env,
                notes: None,
                ready_signal: ReadySignal::Quiet,
                startup_answers: Vec::new(),
                ready_text: None,
                provider: false,
                brief_delivery: BriefDelivery::Paste,
                probe_permissions: false,
                pin_env_via_config: false,
            }
        },
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
        effort_arg: Some(EffortArg::Flag("--effort".into())),
        env,
        notes: None,
        ready_signal: ReadySignal::Status,
        startup_answers: claude_base.startup_answers.clone(),
        ready_text: None,
        provider: true,
        brief_delivery: BriefDelivery::Paste,
        probe_permissions: false,
        pin_env_via_config: false,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeStyle {
    Subcommand(&'static str),
    Flag(&'static str),
}

pub fn resume_style(exe: &str) -> Option<ResumeStyle> {
    Some(match exe {
        "claude" => ResumeStyle::Flag("--resume"),
        "codex" => ResumeStyle::Subcommand("resume"),
        "opencode" => ResumeStyle::Flag("-s"),
        "kimi" => ResumeStyle::Flag("-S"),
        "agy" => ResumeStyle::Flag("--conversation"),
        "pi" => ResumeStyle::Flag("--session"),
        _ => return None,
    })
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

const CLAUDE_MODEL_ALIASES: &[(&str, &str)] = &[
    ("opus", "claude-opus-5-5"),
    ("sonnet", "claude-sonnet-5-5"),
    ("haiku", "claude-haiku-5-5"),
];

pub fn build_command_line(profile: &Profile, model: Option<&str>, effort: Option<Effort>) -> Vec<String> {
    let resolved_model = model.map(|m| {
        if profile.exe == "claude" && !profile.provider {
            CLAUDE_MODEL_ALIASES.iter()
                .find(|(alias, _)| *alias == m)
                .map(|(_, full)| *full)
                .unwrap_or(m)
        } else {
            m
        }
    });
    let mut tokens: Vec<String> = Vec::new();
    tokens.push(profile.exe.clone());
    for arg in &profile.args {
        tokens.push(arg.clone());
    }
    if let (Some(flag), Some(model_val)) = (&profile.model_flag, resolved_model) {
        tokens.push(flag.clone());
        tokens.push(model_val.to_string());
    }
    if let (Some(effort_val), Some(effort_arg)) = (effort, &profile.effort_arg) {
        match effort_arg {
            EffortArg::Flag(flag) => {
                tokens.push(flag.clone());
                tokens.push(effort_val.as_str().to_string());
            }
            EffortArg::ConfigKey(key) => {
                tokens.push("-c".to_string());
                tokens.push(format!("{key}={}", effort_val.as_str()));
            }
        }
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
        assert!(!codex.args.contains(&"--full-auto".to_string()));
        assert!(codex.args.contains(&"check_for_update_on_startup=false".to_string()));
        assert!(codex.probe_permissions);
        assert_eq!(codex.model_flag.as_deref(), Some("-m"));

        let opencode = profiles.iter().find(|p| p.id == "opencode").unwrap();
        assert_eq!(opencode.model_flag.as_deref(), Some("--model"));

        let kimi = profiles.iter().find(|p| p.id == "kimi").unwrap();
        assert_eq!(kimi.model_flag.as_deref(), Some("-m"));
        let update = kimi.startup_answers.iter().find(|a| a.contains == "Continue with current version").unwrap();
        assert_eq!(update.send, "\x1b");

        let pi = profiles.iter().find(|p| p.id == "pi").unwrap();
        assert_eq!(pi.model_flag.as_deref(), Some("--model"));
        let trust = pi.startup_answers.iter().find(|a| a.contains == "Do not trust (this session only)").unwrap();
        assert_eq!(trust.send, "\r");

        let agy = profiles.iter().find(|p| p.id == "antigravity").unwrap();
        assert_eq!(agy.exe, "agy");
        assert!(agy.args.contains(&"--dangerously-skip-permissions".to_string()));
        assert_eq!(agy.model_flag.as_deref(), Some("--model"));
        assert_eq!(agy.env.get("AGY_CLI_DISABLE_AUTO_UPDATE").map(|s| s.as_str()), Some("literal:true"));
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
        let tokens = build_command_line(&p, None, None);
        assert_eq!(tokens, vec!["claude", "--permission-mode", "acceptEdits"]);
    }

    #[test]
    fn command_line_with_model() {
        let p = cli_profile("claude", &["--permission-mode", "acceptEdits"], "--model");
        let tokens = build_command_line(&p, Some("claude-haiku-5-5"), None);
        let cmd = tokens_to_command(&tokens);
        assert!(cmd.contains("--model"));
        assert!(cmd.contains("claude-haiku-5-5"));
    }

    #[test]
    fn command_line_quoting_spaces() {
        let tokens = vec!["exe".to_string(), "value with spaces".to_string()];
        let cmd = tokens_to_command(&tokens);
        assert_eq!(cmd, "exe \"value with spaces\"");
    }

    #[test]
    fn model_alias_resolved_for_claude_non_provider() {
        let p = cli_profile("claude", &[], "--model");
        let tokens = build_command_line(&p, Some("sonnet"), None);
        assert!(tokens.contains(&"claude-sonnet-5-5".to_string()), "sonnet alias must resolve");
        assert!(!tokens.contains(&"sonnet".to_string()), "bare alias must not appear");
    }

    #[test]
    fn model_alias_resolved_for_opus_and_haiku() {
        let p = cli_profile("claude", &[], "--model");
        let opus = build_command_line(&p, Some("opus"), None);
        assert!(opus.contains(&"claude-opus-5-5".to_string()));
        let haiku = build_command_line(&p, Some("haiku"), None);
        assert!(haiku.contains(&"claude-haiku-5-5".to_string()));
    }

    #[test]
    fn model_alias_not_resolved_for_provider_profile() {
        let provider = crate::orch::settings::OrchProvider {
            id: "myprovider".into(),
            preset: "deepseek".into(),
            label: "My Provider".into(),
            base_url: "https://example.com".into(),
            main_model: "fast-model".into(),
            fast_model: "fast-model".into(),
        };
        let p = super::provider_to_profile(&provider);
        let tokens = build_command_line(&p, Some("sonnet"), None);
        assert!(tokens.contains(&"sonnet".to_string()), "provider profile must not map alias");
    }

    #[test]
    fn model_alias_not_resolved_for_non_claude_exe() {
        let p = codex_profile();
        let tokens = build_command_line(&p, Some("haiku"), None);
        assert!(tokens.contains(&"haiku".to_string()), "non-claude exe must not map alias");
    }

    #[test]
    fn codex_flags_follow_help_text() {
        let new_help = "Options:\n  --approve-for-me  auto approve\n  -s, --sandbox <MODE>\n";
        assert_eq!(codex_permission_args(new_help), vec!["--approve-for-me"]);
        let old_help = "Options:\n  -s, --sandbox <MODE>\n  -a, --ask-for-approval <POLICY>\n";
        let args = codex_permission_args(old_help);
        assert_eq!(args, vec!["-s", "workspace-write", "-a", "on-request"]);
        assert!(!args.contains(&"--full-auto".to_string()));
    }

    #[test]
    fn brief_delivery_per_profile() {
        let profiles = built_ins();
        let codex = profiles.iter().find(|p| p.id == "codex").unwrap();
        let claude = profiles.iter().find(|p| p.id == "claude").unwrap();
        assert_eq!(codex.brief_delivery, BriefDelivery::ArgvPointer);
        assert_eq!(claude.brief_delivery, BriefDelivery::Paste);
    }

    #[test]
    fn argv_pointer_quoting() {
        let prompt = brief_pointer_prompt(Path::new("C:/My Dir/it's/brief.md"));
        assert_eq!(prompt, "Read the file C:/My Dir/it's/brief.md and follow it exactly.");
        assert_eq!(
            quote_arg(&prompt, true),
            "'Read the file C:/My Dir/it''s/brief.md and follow it exactly.'"
        );
        assert_eq!(
            quote_arg(&prompt, false),
            "'Read the file C:/My Dir/it'\\''s/brief.md and follow it exactly.'"
        );
    }

    #[test]
    fn pinned_env_args_quote_for_both_shells() {
        let env = vec![
            ("PANORAMA_CLI".to_string(), r"C:\Users\me\cli.exe".to_string()),
            ("ANTHROPIC_AUTH_TOKEN".to_string(), "secret".to_string()),
            ("PANORAMA_NOTE".to_string(), r#"a"b"#.to_string()),
        ];
        let ps = pinned_env_args(&env, true);
        assert_eq!(ps, vec![
            "-c".to_string(),
            r#"'shell_environment_policy.set.PANORAMA_CLI="C:\\Users\\me\\cli.exe"'"#.to_string(),
            "-c".to_string(),
            r#"'shell_environment_policy.set.PANORAMA_NOTE="a\"b"'"#.to_string(),
        ]);
        let sh = pinned_env_args(&env[..1], false);
        assert_eq!(sh[1], r#"'shell_environment_policy.set.PANORAMA_CLI="C:\\Users\\me\\cli.exe"'"#);
        assert!(!ps.iter().any(|a| a.contains("ANTHROPIC")));
    }

    #[test]
    fn codex_pins_env_and_answers_hooks_dialog() {
        let codex = built_ins().into_iter().find(|p| p.id == "codex").unwrap();
        assert!(codex.pin_env_via_config);
        let answer = codex.startup_answers.iter().find(|a| a.contains == "Hooks need review").unwrap();
        assert_eq!(answer.send, "2\r");
    }

    #[test]
    fn codex_answers_trust_folder_dialog_with_enter() {
        let codex = built_ins().into_iter().find(|p| p.id == "codex").unwrap();
        let screen = "Trust this folder? Codex can read, edit, and run files here.\n> 1. Trust and continue\n  2. Back to Agent Command Center\nenter continue - esc back";
        let answer = codex.startup_answers.iter().find(|a| screen.contains(&a.contains)).unwrap();
        assert_eq!(answer.send, "\r");
    }

    #[test]
    fn model_alias_passthrough_for_full_id() {
        let p = cli_profile("claude", &[], "--model");
        let tokens = build_command_line(&p, Some("claude-sonnet-5-5"), None);
        assert!(tokens.contains(&"claude-sonnet-5-5".to_string()), "full id must pass through unchanged");
    }

    #[test]
    fn command_line_effort_args_per_profile() {
        let profiles = built_ins();
        let find = |id: &str| profiles.iter().find(|p| p.id == id).unwrap();

        let claude = build_command_line(find("claude"), Some("sonnet"), Some(Effort::High));
        assert!(claude.windows(2).any(|w| w == ["--effort", "high"]), "{claude:?}");
        let claude_none = build_command_line(find("claude"), Some("sonnet"), None);
        assert!(!claude_none.contains(&"--effort".to_string()));

        let codex = build_command_line(find("codex"), Some("o3"), Some(Effort::Xhigh));
        assert!(codex.windows(2).any(|w| w == ["-c", "model_reasoning_effort=xhigh"]), "{codex:?}");
        let codex_none = build_command_line(find("codex"), Some("o3"), None);
        assert!(!codex_none.iter().any(|t| t.contains("model_reasoning_effort")));

        let pi = build_command_line(find("pi"), Some("m"), Some(Effort::Medium));
        assert!(pi.windows(2).any(|w| w == ["--thinking", "medium"]), "{pi:?}");

        let agy = build_command_line(find("antigravity"), Some("m"), Some(Effort::Low));
        assert!(agy.windows(2).any(|w| w == ["--effort", "low"]), "{agy:?}");

        let kimi = build_command_line(find("kimi"), Some("k"), Some(Effort::Max));
        assert_eq!(kimi, build_command_line(find("kimi"), Some("k"), None));

        let opencode = build_command_line(find("opencode"), Some("p/m"), Some(Effort::High));
        assert_eq!(opencode, build_command_line(find("opencode"), Some("p/m"), None));
    }
}
