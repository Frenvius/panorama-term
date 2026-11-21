use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::orch::cli_env::{cli_command, kill_tree};
use crate::orch::settings::{AiActions, AiTarget, OrchSettings};

pub struct OneShotOpts {
    pub provider: String,
    pub model: String,
    pub prompt: String,
}

struct ProcOutput {
    stdout: String,
    stderr: String,
    exit_code: Option<i32>,
    timed_out: bool,
}

fn run_proc(mut cmd: std::process::Command, stdin_input: &str) -> Result<ProcOutput, String> {
    use std::io::Read;
    use std::process::Stdio;

    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("cli_missing:{e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_input.as_bytes());
    }

    let mut stdout_pipe = child.stdout.take().expect("stdout pipe");
    let mut stderr_pipe = child.stderr.take().expect("stderr pipe");

    let stdout_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout_pipe.read_to_string(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr_pipe.read_to_string(&mut buf);
        buf
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    let (exit_code, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(s)) => break (s.code(), false),
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    kill_tree(&mut child);
                    break (None, true);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                kill_tree(&mut child);
                return Err(format!("io:{e}"));
            }
        }
    };

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    Ok(ProcOutput { stdout, stderr, exit_code, timed_out })
}

fn tmp_path(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!("panorama-{prefix}-{nanos}.txt"))
}

fn stderr_tail(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().rev().take(5).collect();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

pub fn run_oneshot(opts: OneShotOpts) -> Result<String, String> {
    let Some(base) = crate::panorama_dir() else {
        return Err("io:no panorama dir".to_string());
    };
    match opts.provider.as_str() {
        "claude" => run_oneshot_claude(&opts.model, &opts.prompt),
        "codex" => run_oneshot_codex(&opts.model, &opts.prompt),
        id => run_oneshot_custom(id, &opts.model, &opts.prompt, &base),
    }
}

fn run_oneshot_claude(model: &str, prompt: &str) -> Result<String, String> {
    let mut cmd = cli_command("claude");
    cmd.args([
        "-p", "--output-format", "text",
        "--tools", "", "--no-session-persistence", "--strict-mcp-config",
    ]);
    if !model.is_empty() {
        cmd.args(["--model", model]);
    }
    cmd.env("CLAUDE_CODE_PLUGIN_DIRS", "");
    let out = run_proc(cmd, prompt)?;
    if out.timed_out {
        return Err("timeout:claude oneshot timed out after 120s".to_string());
    }
    if out.exit_code == Some(0) {
        Ok(out.stdout)
    } else {
        Err(format!("nonzero_exit:{}", stderr_tail(&out.stderr)))
    }
}

fn run_oneshot_codex(model: &str, prompt: &str) -> Result<String, String> {
    let out_file = tmp_path("codex-out");
    let out_path = out_file.to_string_lossy().into_owned();

    let mut cmd = cli_command("codex");
    cmd.args([
        "exec",
        "--skip-git-repo-check",
        "-s", "read-only",
        "--ephemeral",
        "-o", &out_path,
    ]);
    if !model.is_empty() {
        cmd.args(["-m", model]);
    }

    let proc = run_proc(cmd, prompt)?;

    if proc.timed_out {
        let _ = std::fs::remove_file(&out_file);
        return Err("timeout:codex oneshot timed out after 120s".to_string());
    }
    if proc.exit_code != Some(0) {
        let _ = std::fs::remove_file(&out_file);
        return Err(format!("nonzero_exit:{}", stderr_tail(&proc.stderr)));
    }

    let content = std::fs::read_to_string(&out_file)
        .map_err(|e| format!("io:read output file: {e}"))?;
    let _ = std::fs::remove_file(&out_file);
    Ok(content)
}

fn run_oneshot_custom(provider_id: &str, model: &str, prompt: &str, base: &Path) -> Result<String, String> {
    let profiles = crate::orch::profiles::load_profiles(base);
    let profile = crate::orch::profiles::find_profile(&profiles, provider_id)
        .ok_or_else(|| format!("not_found:provider not found: {provider_id}"))?;

    let resolved_env = crate::orch::profiles::resolve_env(&profile.env, Some(base));

    let mut cmd = cli_command("claude");
    cmd.args([
        "-p", "--output-format", "text",
        "--tools", "", "--no-session-persistence", "--strict-mcp-config",
    ]);
    if !model.is_empty() {
        cmd.args(["--model", model]);
    }
    cmd.env("CLAUDE_CODE_PLUGIN_DIRS", "");
    for (k, v) in resolved_env {
        cmd.env(k, v);
    }

    let out = run_proc(cmd, prompt)?;
    if out.timed_out {
        return Err("timeout:oneshot timed out after 120s".to_string());
    }
    if out.exit_code == Some(0) {
        Ok(out.stdout)
    } else {
        Err(format!("nonzero_exit:{}", stderr_tail(&out.stderr)))
    }
}

pub(crate) fn resolve_ai_target(settings: &OrchSettings, action: Option<&str>) -> (String, String) {
    let AiActions { default: def, overrides: ov } = &settings.ai_actions;
    let target: Option<&AiTarget> = match action {
        Some("commitMessage") => ov.commit_message.as_ref(),
        Some("tidy") => ov.tidy.as_ref(),
        _ => None,
    };
    match target {
        Some(t) => (t.provider.clone(), t.model.clone()),
        None => (def.provider.clone(), def.model.clone()),
    }
}

pub fn clean_commit_message(raw: &str) -> String {
    raw.lines()
        .map(|l| l.trim().trim_matches('"').trim_matches('`').trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

pub fn extract_json_object(raw: &str) -> Result<String, String> {
    let bytes = raw.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')
        .ok_or_else(|| "no JSON object in response".to_string())?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape_next = false;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if escape_next {
            escape_next = false;
        } else if in_string {
            match b {
                b'"' => in_string = false,
                b'\\' => escape_next = true,
                _ => {}
            }
        } else {
            match b {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(raw[start..=i].to_string());
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    Err("unbalanced JSON object in response".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::settings::{AiActionOverrides, AiActions, AiTarget, OrchSettings};

    #[test]
    fn clean_commit_strips_backticks() {
        assert_eq!(clean_commit_message("`feat: add login`"), "feat: add login");
    }

    #[test]
    fn clean_commit_strips_quotes() {
        assert_eq!(clean_commit_message("\"fix: broken auth\""), "fix: broken auth");
    }

    #[test]
    fn clean_commit_takes_first_nonempty_line() {
        let raw = "\n\nfeat: add search\n\nSome explanation";
        assert_eq!(clean_commit_message(raw), "feat: add search");
    }

    #[test]
    fn clean_commit_strips_whitespace() {
        assert_eq!(clean_commit_message("  fix: typo  "), "fix: typo");
    }

    #[test]
    fn extract_json_simple() {
        let raw = r#"Here is the plan: {"projectOrder":["a"],"frames":[]}"#;
        let json = extract_json_object(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["projectOrder"][0], "a");
    }

    #[test]
    fn extract_json_fenced_block() {
        let raw = "Some prose\n```json\n{\"projectOrder\":[],\"frames\":[]}\n```\nMore text";
        let json = extract_json_object(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["frames"].as_array().unwrap().is_empty());
    }

    #[test]
    fn extract_json_nested() {
        let raw = r#"{"a":{"b":1},"c":[]}"#;
        let json = extract_json_object(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["a"]["b"], 1);
    }

    #[test]
    fn extract_json_string_with_braces() {
        let raw = r#"{"key":"value with {braces} inside"}"#;
        let json = extract_json_object(raw).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["key"].as_str().unwrap(), "value with {braces} inside");
    }

    #[test]
    fn extract_json_no_object_errors() {
        assert!(extract_json_object("no json here").is_err());
    }

    #[test]
    fn resolve_target_uses_default_when_no_override() {
        let s = OrchSettings {
            ai_actions: AiActions {
                default: AiTarget { provider: "claude".into(), model: "sonnet".into() },
                overrides: AiActionOverrides::default(),
            },
            ..OrchSettings::default()
        };
        let (p, m) = resolve_ai_target(&s, Some("commitMessage"));
        assert_eq!(p, "claude");
        assert_eq!(m, "sonnet");
    }

    #[test]
    fn resolve_target_uses_override_when_present() {
        let s = OrchSettings {
            ai_actions: AiActions {
                default: AiTarget { provider: "claude".into(), model: "haiku".into() },
                overrides: AiActionOverrides {
                    commit_message: Some(AiTarget { provider: "codex".into(), model: "o3".into() }),
                    tidy: None,
                },
            },
            ..OrchSettings::default()
        };
        let (p, m) = resolve_ai_target(&s, Some("commitMessage"));
        assert_eq!(p, "codex");
        assert_eq!(m, "o3");
    }

    #[test]
    fn resolve_target_tidy_override_independent() {
        let s = OrchSettings {
            ai_actions: AiActions {
                default: AiTarget { provider: "claude".into(), model: "haiku".into() },
                overrides: AiActionOverrides {
                    commit_message: None,
                    tidy: Some(AiTarget { provider: "claude".into(), model: "opus".into() }),
                },
            },
            ..OrchSettings::default()
        };
        let (p, m) = resolve_ai_target(&s, Some("tidy"));
        assert_eq!(p, "claude");
        assert_eq!(m, "opus");
        let (p2, m2) = resolve_ai_target(&s, Some("commitMessage"));
        assert_eq!(p2, "claude");
        assert_eq!(m2, "haiku");
    }

    #[test]
    #[ignore]
    fn ai_oneshot_real_claude_haiku() {
        let out = run_oneshot(OneShotOpts {
            provider: "claude".into(),
            model: "haiku".into(),
            prompt: "Reply with exactly the words: hello world".into(),
        })
        .expect("oneshot should succeed");
        eprintln!("real claude haiku output:\n{out}");
        assert!(!out.trim().is_empty());
        assert!(out.to_lowercase().contains("hello"));
    }

    #[test]
    #[ignore]
    fn ai_oneshot_real_codex() {
        let out = run_oneshot(OneShotOpts {
            provider: "codex".into(),
            model: String::new(),
            prompt: "Reply with exactly the words: hello world".into(),
        })
        .expect("oneshot should succeed");
        eprintln!("real codex output:\n{out}");
        assert!(out.to_lowercase().contains("hello"));
    }
}
