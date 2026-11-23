use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::orch::model::{Task, Dispatch};
use crate::orch::profiles::{
    BriefDelivery, Profile, ReadySignal, brief_pointer_prompt, build_command_line, codex_permission_args, pinned_env_args, quote_arg,
    tokens_to_command,
};
use crate::{Params, get_or_create, send_to_tile, host_handle, sessions, lookup_live, cached_agent_state};
use crate::ui::{broadcast_grouped_tile_open, TileGroup};

const SHELL_SETTLE_MS: u64 = 800;
const AGENT_READY_IDLE_TIMEOUT_S: u64 = 90;
const AGENT_QUIET_WAIT_S: u64 = 4;
const AGENT_QUIET_DURATION_S: u64 = 3;
const STARTUP_ANSWER_QUIET_MS: u64 = 1000;
pub const CLI_PLACEHOLDER: &str = "{cli}";
const STARTUP_ANSWER_MAX_SENDS: u8 = 3;
const NUDGE_COOLDOWN_S: u64 = 30;
const EXITED_SHELL_POLLS: u8 = 6;
const EXITED_MIN_WAIT_S: u64 = 2;
const ARGV_EARLY_EXIT_WATCH_S: u64 = 15;
const WORKER_COLS: u16 = 120;
const WORKER_ROWS: u16 = 30;

fn nudge_times() -> &'static Mutex<HashMap<String, Instant>> {
    static MAP: OnceLock<Mutex<HashMap<String, Instant>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Clone, Copy)]
pub struct BriefInput<'a> {
    pub base: &'a Path,
    pub run_id: &'a str,
    pub dispatch_id: &'a str,
    pub task: &'a Task,
    pub dispatch: &'a Dispatch,
    pub deps_results: &'a [(String, Option<String>)],
    pub cli_path: &'a str,
    pub cwd: &'a str,
    pub merge_note: Option<&'a str>,
    pub handoff: Option<&'a str>,
    pub correction: Option<&'a Dispatch>,
}

pub struct WorkerLaunch<'a> {
    pub tile_id: &'a str,
    pub cwd: &'a str,
    pub group: Option<TileGroup<'a>>,
    pub profile: &'a Profile,
    pub model: Option<&'a str>,
    pub extra_env: Vec<(String, String)>,
    pub brief_path: &'a Path,
    pub settings_path: Option<String>,
    pub resume_id: Option<&'a str>,
}

const ALLOWED_WORKER_COMMANDS: &[&str] = &[
    "cargo test:*",
    "bun test:*",
    "bun x tsc:*",
    "codex exec --help",
    "claude --help",
];

pub fn build_worker_settings(verify_commands: &[String]) -> serde_json::Value {
    let mut allow: Vec<String> = vec!["Read(~/.claude/skills/**)".into()];
    let commands = ALLOWED_WORKER_COMMANDS.iter().map(|c| c.to_string()).chain(verify_commands.iter().cloned());
    for cmd in commands {
        for tool in ["Bash", "PowerShell"] {
            let rule = format!("{tool}({cmd})");
            if !allow.contains(&rule) {
                allow.push(rule);
            }
        }
    }
    serde_json::json!({ "permissions": { "allow": allow } })
}

pub fn write_worker_settings(brief_path: &Path, verify_commands: &[String]) -> Result<PathBuf, String> {
    let path = brief_path.with_file_name("claude-settings.json");
    let json = build_worker_settings(verify_commands);
    let text = serde_json::to_string_pretty(&json).map_err(|e| format!("serialize worker settings: {e}"))?;
    std::fs::write(&path, text).map_err(|e| format!("write worker settings: {e}"))?;
    Ok(path)
}

pub fn conflict_brief(branch: &str, files: &[String], integrated_titles: &[String]) -> String {
    let mut note = String::from("## Resolve merge conflict\n\n");
    note.push_str(&format!(
        "The work for this task is already done on `{branch}`. Its merge into the run's integration branch conflicted, so `git merge --no-ff --no-commit {branch}` is in progress in your working directory.\n\n"
    ));
    if files.is_empty() {
        note.push_str("The merge applied cleanly; check that the result builds and passes the verify commands.\n\n");
    } else {
        note.push_str("Resolve the conflict markers in these files, keeping both sides' intent:\n\n");
        for f in files {
            note.push_str(&format!("- `{f}`\n"));
        }
        note.push('\n');
    }
    if !integrated_titles.is_empty() {
        note.push_str("The integration branch side contains these already integrated tasks:\n\n");
        for t in integrated_titles {
            note.push_str(&format!("- {t}\n"));
        }
        note.push('\n');
    }
    note.push_str("Do not change anything else. Do not abort the merge or run git commit. Report when done.\n\n");
    note
}

fn report_instruction(cli_path: &str) -> String {
    format!("Pass two outputs. Use --summary for a short handoff with Status, Result (one or two sentences), Verification (what ran and its outcome), and Blocker if any. Put each field on its own line. For a failed outcome, start with Blocker: and give the failure reason. Use --report-file for the full report and repeat --file for each changed file.\n\nReport exactly once when finished:\n```\n{cli_path} orch send --type worker_done --outcome succeeded|failed --summary \"...\" --report-file <temp-file> [--file path ...]\n```\n\n")
}

fn corrective_message(previous: &Dispatch, cli_path: &str) -> String {
    let mut message = String::from("Continue the task in this worktree. ");
    if let Some(failed) = previous.verifications.iter().rev().find(|v| v.timed_out || v.exit_code != Some(0)) {
        let outcome = if failed.timed_out { "timed out".to_string() } else {
            failed.exit_code.map_or_else(|| "could not run".into(), |code| format!("exited with code {code}"))
        };
        message.push_str(&format!("Verification {} {outcome}. Fix the failure and rerun the check.\n\n", failed.command));
        for line in failed.tail.iter().rev().take(12).rev() {
            message.extend(line.chars().take(400).filter(|c| !c.is_control()));
            message.push('\n');
        }
    } else {
        message.push_str("The operational wait has ended. Finish the task and run its checks.\n");
    }
    message.push_str("\nUse the current PANORAMA_RUN_ID and PANORAMA_DISPATCH_ID. Write a full report with the changes and checks. Leave changes in the worktree and do not commit.\n\n");
    message.push_str(&report_instruction(cli_path));
    message
}

pub fn write_brief(input: BriefInput) -> Result<PathBuf, String> {
    let BriefInput { base, run_id, dispatch_id, task, dispatch, deps_results, cli_path, cwd, merge_note, handoff, correction } = input;
    let dispatch_dir = base
        .join("orch")
        .join("runs")
        .join(run_id)
        .join("dispatch")
        .join(dispatch_id);
    std::fs::create_dir_all(&dispatch_dir)
        .map_err(|e| format!("create dispatch dir: {e}"))?;

    let brief_path = dispatch_dir.join("brief.md");
    if let Some(previous) = correction {
        std::fs::write(&brief_path, corrective_message(previous, cli_path))
            .map_err(|e| format!("write correction: {e}"))?;
        return Ok(brief_path);
    }

    let mut content = String::from("Read CLAUDE.md and AGENTS.md at the worktree root if they exist, and follow their conventions.\nIf those instructions conflict with the task spec below, the task spec wins.\n\n");
    content.push_str(&format!("# Task: {}\n\n", task.title));
    content.push_str(&format!("## Spec\n\n{}\n\n", task.spec));

    if let Some(ref acceptance) = task.acceptance {
        content.push_str(&format!("## Acceptance criteria\n\n{acceptance}\n\n"));
    }

    if !task.verify.is_empty() {
        content.push_str("## Verify commands\n\n");
        for cmd in &task.verify {
            content.push_str(&format!("- `{cmd}`\n"));
        }
        content.push('\n');
    }

    if !deps_results.is_empty() {
        content.push_str("## Completed dependency results\n\n");
        for (dep_id, result) in deps_results {
            if let Some(r) = result {
                content.push_str(&format!("- **{dep_id}**: {r}\n"));
            }
        }
        content.push('\n');
    }

    if let Some(block) = handoff {
        content.push_str(block);
        content.push_str("\n\n");
    }

    content.push_str(&format!("## Working directory\n\n`{cwd}`\n\n"));

    if let Some(note) = merge_note {
        content.push_str(note);
    }

    let attempt = dispatch.attempt;
    if attempt > 1 {
        content.push_str(&format!("## Attempt\n\nThis is attempt {attempt}.\n\n"));
    }

    content.push_str("## Contract\n\n");
    content.push_str("`PANORAMA_RUN_ID` and `PANORAMA_DISPATCH_ID` are already set in your environment; no ids are needed in commands.\n\n");
    content.push_str("Run each command below as written, on its own: no `cd`/`Set-Location` before it, nothing appended after it. Only that exact form is pre-approved; anything else stops you on a permission prompt nobody may be watching.\n\n");
    content.push_str("Provide a full report for every role. Explore and review reports must include all findings with file:line evidence; leave nothing only in chat. Implement, test and mechanical reports must explain what changed, why, the root cause for fixes, what was verified and what was skipped. Write the full report to a temporary file outside the worktree.\n\n");
    content.push_str(&report_instruction(cli_path));
    content.push_str(&format!(
        "Blocking question:\n```\n{cli_path} orch send --type question --body \"...\"\n{cli_path} orch check --wait --timeout-ms 600000\n```\n\n"
    ));
    content.push_str(&format!(
        "Optional progress:\n```\n{cli_path} orch send --type progress --body \"...\"\n```\n\n"
    ));
    content.push_str("Leave your changes in the working directory; Panorama commits them when you report. Do not run git commit.\n");
    content.push_str("Do not change files outside the working directory except the temporary report file. Do not push.\n");

    std::fs::write(&brief_path, &content)
        .map_err(|e| format!("write brief: {e}"))?;

    Ok(brief_path)
}

pub(super) fn pin_native_home(exe: &str, tile_id: &str, env: &mut Vec<(String, String)>) -> Option<String> {
    let key = match exe {
        "claude" => "CLAUDE_CONFIG_DIR",
        "codex" => "CODEX_HOME",
        _ => return None,
    };
    let matches_key = |name: &str| name == key || (cfg!(windows) && name.eq_ignore_ascii_case(key));
    let selected = env.iter().rev().find(|(name, _)| matches_key(name)).map(|(_, path)| path.clone())
        .or_else(|| crate::accounts::tile_env(tile_id).into_iter().find(|(name, _)| matches_key(name)).map(|(_, path)| path))
        .or_else(|| std::env::var(key).ok());
    let home = match selected {
        Some(path) => PathBuf::from(path),
        None if exe == "claude" => crate::claude_home()?,
        None => crate::codex_home()?,
    };
    if !home.is_absolute() {
        return None;
    }
    let home = home.to_str()?.to_string();
    env.retain(|(name, _)| !matches_key(name));
    env.push((key.into(), home.clone()));
    Some(home)
}

pub(super) fn valid_session_id(id: &str) -> bool {
    id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
}

pub async fn spawn_worker(launch: WorkerLaunch<'_>) -> Result<(), String> {
    let cli = crate::panorama_cli_path();
    let mut pinned_env = launch.extra_env.clone();
    pinned_env.push(("PANORAMA_CLI".into(), cli.clone()));
    pinned_env.push(("PANORAMA_SIDECAR_PORT".into(), crate::port().to_string()));
    pinned_env.push(("PANORAMA_TILE_ID".into(), launch.tile_id.into()));
    let command = worker_command(&launch, &cli, &pinned_env);
    let WorkerLaunch { tile_id, cwd, group, profile, extra_env, brief_path, resume_id, .. } = launch;
    let params = Params {
        tile_id: tile_id.to_string(),
        cols: WORKER_COLS,
        rows: WORKER_ROWS,
        cwd: Some(cwd.to_string()),
        shell: None,
        target: None,
        elevated: false,
        idle_compact: true,
        spawn_cmd: None,
        run_kind: "worker".into(),
        attach_only: false,
        watch_only: false,
        remote: false,
        extra_env,
    };

    get_or_create(&params).await?;

    let show_tiles = crate::panorama_dir()
        .map(|base| crate::orch::settings::load(&base).show_worker_tiles)
        .unwrap_or(false);
    if show_tiles {
        broadcast_grouped_tile_open(tile_id, cwd, group);
    }

    tokio::time::sleep(Duration::from_millis(SHELL_SETTLE_MS)).await;

    if profile.exe == "agy" {
        crate::orch::agy::ensure_trusted_workspace(cwd);
    }

    send_to_tile(tile_id, &command)?;

    if resume_id.is_none() && profile.brief_delivery == BriefDelivery::ArgvPointer {
        return watch_argv_startup(tile_id, profile).await;
    }
    wait_for_agent_ready(tile_id, profile, resume_id).await?;
    let brief = std::fs::read_to_string(brief_path).map_err(|e| format!("read brief: {e}"))?;
    send_to_tile(tile_id, &brief)
}

fn worker_command(launch: &WorkerLaunch<'_>, cli: &str, pinned_env: &[(String, String)]) -> String {
    let WorkerLaunch { profile, model, settings_path, resume_id, brief_path, .. } = launch;
    let mut tokens: Vec<String> = build_command_line(profile, *model)
        .into_iter()
        .map(|t| t.replace(CLI_PLACEHOLDER, cli))
        .collect();
    if profile.probe_permissions {
        let flags = codex_permission_args(&codex_help());
        tokens.splice(1..1, flags);
    }
    if let Some(sp) = settings_path.as_ref().filter(|_| profile.exe == "claude") {
        tokens.push("--settings".to_string());
        tokens.push(sp.clone());
    }
    if resume_id.is_some() && profile.exe == "codex" {
        tokens.insert(1, "resume".into());
    }
    let mut command = tokens_to_command(&tokens);
    if profile.pin_env_via_config {
        for arg in pinned_env_args(pinned_env, cfg!(windows)) {
            command.push(' ');
            command.push_str(&arg);
        }
    }
    if let Some(id) = resume_id {
        if profile.exe == "claude" {
            command.push_str(" --resume");
        }
        command.push(' ');
        command.push_str(id);
    } else if profile.brief_delivery == BriefDelivery::ArgvPointer {
        command.push(' ');
        command.push_str(&quote_arg(&brief_pointer_prompt(brief_path), cfg!(windows)));
    }
    command
}

const CODEX_HELP_TIMEOUT_S: u64 = 8;

fn codex_help() -> String {
    static HELP: OnceLock<String> = OnceLock::new();
    HELP.get_or_init(probe_codex_help).clone()
}

fn probe_codex_help() -> String {
    let exe = crate::panorama_dir()
        .and_then(|d| crate::original_codex(&d.join("bin")))
        .unwrap_or_else(|| "codex".to_string());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut cmd = crate::hidden_command(&exe);
        cmd.arg("--help");
        if let Some(path) = crate::orch::cli_env::cli_path() {
            cmd.env("PATH", path);
        }
        let _ = tx.send(cmd.output().ok());
    });
    match rx.recv_timeout(Duration::from_secs(CODEX_HELP_TIMEOUT_S)) {
        Ok(Some(out)) => format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
        _ => String::new(),
    }
}

fn agent_back_at_shell(tile_id: &str) -> bool {
    let Some(session) = sessions().lock().unwrap().get(tile_id).cloned() else { return false };
    if crate::session_pid(&session) == 0 {
        return false;
    }
    let shell = crate::shell_name_for(&session);
    crate::foreground_name_for(&session).map_or(true, |fg| fg == shell)
}

// TUI agents drop a key when it arrives in the same read as Enter, so Enter goes separately.
async fn send_keys(session: &std::sync::Arc<crate::Session>, keys: &str) {
    let (before, submit) = match keys.strip_suffix('\r') {
        Some(rest) => (rest, true),
        None => (keys, false),
    };
    if !before.is_empty() {
        crate::write_input(session, before);
    }
    if submit {
        tokio::time::sleep(Duration::from_millis(250)).await;
        crate::write_input(session, "\r");
    }
}

fn exited_during_startup(shell_polls: u8, since_launch: Duration) -> bool {
    shell_polls >= EXITED_SHELL_POLLS && since_launch >= Duration::from_secs(EXITED_MIN_WAIT_S)
}

fn exited_error(tile_id: &str) -> String {
    let tail = capture_worker_screen(tile_id, 10);
    format!("agent exited during startup; screen: {}", tail.trim())
}

fn codex_trust_dialog(screen: &str) -> bool {
    screen.contains("Trust this folder?") && screen.contains("Trust and continue")
}

async fn answer_startup_prompt(tile_id: &str, profile: &Profile, answer_sends: &mut [u8]) -> bool {
    let screen = capture_worker_screen(tile_id, 40);
    let pending = profile.startup_answers.iter().zip(answer_sends.iter_mut())
        .find(|(answer, sends)| {
            if profile.id == "codex" && answer.contains == "Trust this folder?" {
                **sends == 0 && codex_trust_dialog(&screen)
            } else {
                **sends < STARTUP_ANSWER_MAX_SENDS && screen.contains(&answer.contains)
            }
        });
    let Some((answer, sends)) = pending else { return false };
    *sends += 1;
    if let Some(s) = lookup_live(tile_id) {
        send_keys(&s, &answer.send).await;
    }
    true
}

async fn watch_argv_startup(tile_id: &str, profile: &Profile) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(ARGV_EARLY_EXIT_WATCH_S);
    let launch_time = Instant::now();
    let mut last_activity = Instant::now();
    let mut last_snapshot_len = 0usize;
    let mut answer_sends = vec![0u8; profile.startup_answers.len()];
    let mut shell_polls = 0u8;

    while Instant::now() < deadline {
        let snapshot_len = host_handle().snapshot(tile_id).len();
        if snapshot_len != last_snapshot_len {
            last_snapshot_len = snapshot_len;
            last_activity = Instant::now();
        }
        let quiet_duration = Instant::now().duration_since(last_activity);
        if quiet_duration >= Duration::from_millis(STARTUP_ANSWER_QUIET_MS)
            && answer_startup_prompt(tile_id, profile, &mut answer_sends).await
        {
            last_activity = Instant::now();
        }

        shell_polls = if agent_back_at_shell(tile_id) { shell_polls.saturating_add(1) } else { 0 };
        if exited_during_startup(shell_polls, Instant::now().duration_since(launch_time)) {
            return Err(exited_error(tile_id));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    Ok(())
}

async fn wait_for_agent_ready(tile_id: &str, profile: &Profile, resume_id: Option<&str>) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(AGENT_READY_IDLE_TIMEOUT_S);
    let launch_time = Instant::now();
    let mut last_activity = Instant::now();
    let mut last_snapshot_len = 0usize;
    let mut answer_sends = vec![0u8; profile.startup_answers.len()];
    let mut shell_polls = 0u8;

    loop {
        if Instant::now() >= deadline {
            let tail = capture_worker_screen(tile_id, 6);
            return Err(format!("agent did not start; screen: {}", tail.trim()));
        }

        let snapshot_len = host_handle().snapshot(tile_id).len();
        if snapshot_len != last_snapshot_len {
            last_snapshot_len = snapshot_len;
            last_activity = Instant::now();
        }
        let quiet_duration = Instant::now().duration_since(last_activity);

        if quiet_duration >= Duration::from_millis(STARTUP_ANSWER_QUIET_MS)
            && answer_startup_prompt(tile_id, profile, &mut answer_sends).await
        {
            last_activity = Instant::now();
            tokio::time::sleep(Duration::from_millis(300)).await;
            continue;
        }

        let bound = resume_id.is_none_or(|id| {
            crate::read_binding_rec(tile_id).is_some_and(|binding|
                binding["agent"] == profile.exe && binding["agentSessionId"] == id)
        });
        if let Some(s) = lookup_live(tile_id).filter(|_| bound) {
            if let Some(state_str) = cached_agent_state(&s) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&state_str) {
                    if v.get("lastStatus").and_then(|s| s.as_str()) == Some("idle") {
                        return Ok(());
                    }
                }
            }
        }

        let since_launch = Instant::now().duration_since(launch_time);
        shell_polls = if agent_back_at_shell(tile_id) { shell_polls.saturating_add(1) } else { 0 };
        if exited_during_startup(shell_polls, since_launch) {
            return Err(exited_error(tile_id));
        }
        if bound && profile.ready_signal == ReadySignal::Quiet
            && since_launch >= Duration::from_secs(AGENT_QUIET_WAIT_S)
            && quiet_duration >= Duration::from_secs(AGENT_QUIET_DURATION_S)
            && ready_text_visible(tile_id, profile)
        {
            return Ok(());
        }

        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn ready_text_visible(tile_id: &str, profile: &Profile) -> bool {
    profile.ready_text.as_ref().map_or(true, |text| capture_worker_screen(tile_id, 40).contains(text.as_str()))
}

pub fn capture_worker_history(tile_id: &str) -> Result<String, String> {
    let session = sessions().lock().unwrap().get(tile_id).cloned().ok_or("worker session not found")?;
    let mut parser = session.parser.lock().unwrap_or_else(|e| e.into_inner());
    let disk = parser.callbacks_mut().scroll_store.as_mut().map(|store| {
        let (path, _, _, end) = store.snapshot();
        std::fs::read(path).map(|mut bytes| { bytes.truncate(end as usize); bytes })
    }).transpose().map_err(|e| e.to_string())?.unwrap_or_default();
    let mut text = String::from_utf8_lossy(&disk).lines()
        .map(|line| vt100::capture::basic_formatted_to_plain(line).collect::<String>())
        .collect::<Vec<_>>().join("\n");
    let (rows, cols) = crate::grid_size(parser.screen());
    let raw = crate::history::main_history(parser.screen_mut());
    let rest = crate::capture_text(&raw, rows, cols, usize::MAX);
    if !text.is_empty() && !rest.is_empty() { text.push('\n'); }
    text.push_str(&rest);
    if parser.screen().alternate_screen() {
        let visible = parser.screen().contents();
        if !visible.is_empty() {
            if !text.is_empty() { text.push('\n'); }
            text.push_str(&visible);
        }
    }
    Ok(text)
}

pub fn capture_worker_screen(tile_id: &str, lines: usize) -> String {
    let lines = lines.min(5000).max(1);
    let (raw, rows, cols) = match sessions().lock().unwrap().get(tile_id).cloned() {
        Some(s) => (
            host_handle().snapshot(tile_id),
            s.rows.load(std::sync::atomic::Ordering::Relaxed),
            s.cols.load(std::sync::atomic::Ordering::Relaxed),
        ),
        None => match host_handle().session_info(tile_id) {
            Some((true, rows, cols)) => (host_handle().snapshot(tile_id), rows, cols),
            _ => (Vec::new(), WORKER_ROWS, WORKER_COLS),
        },
    };
    crate::capture_text(&raw, rows, cols, lines)
}

#[derive(Debug, PartialEq)]
enum NudgeDecision { Skip, Now, Defer(Duration) }

fn nudge_decision(idle: bool, unacked: usize, elapsed: Option<Duration>, replaying: bool) -> NudgeDecision {
    if replaying || !idle || unacked == 0 { return NudgeDecision::Skip; }
    match elapsed.and_then(|d| Duration::from_secs(NUDGE_COOLDOWN_S).checked_sub(d)) {
        Some(delay) if !delay.is_zero() => NudgeDecision::Defer(delay),
        _ => NudgeDecision::Now,
    }
}

pub(super) fn maybe_nudge_tile(tile_id: &str, run_id: &str, reason: super::WakeReason, n: usize, detached: bool) {
    let session = sessions().lock().unwrap().get(tile_id).cloned();
    let idle = !is_tile_waiting(tile_id) && session.as_ref().and_then(|s| cached_agent_state(s))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["lastStatus"].as_str().map(|s| s == "idle")).unwrap_or(false);
    nudge_coordinator(tile_id, run_id, reason, n, detached, idle);
}

fn nudge_coordinator(tile_id: &str, run_id: &str, reason: super::WakeReason, n: usize, detached: bool, idle: bool) {
    static PENDING: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let mut times = nudge_times().lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    match nudge_decision(idle, n, times.get(tile_id).map(|t| now.duration_since(*t)), false) {
        NudgeDecision::Skip => return,
        NudgeDecision::Now => { times.insert(tile_id.to_string(), now); }
        NudgeDecision::Defer(delay) => {
            let pending = PENDING.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
            if pending.lock().unwrap_or_else(|e| e.into_inner()).insert(tile_id.to_string()) {
                let tile = tile_id.to_string();
                let run = run_id.to_string();
                if let Ok(handle) = tokio::runtime::Handle::try_current() {
                    handle.spawn(async move {
                        tokio::time::sleep(delay).await;
                        pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&tile);
                        crate::orch::recheck_coordinator(&run, &tile);
                    });
                } else { pending.lock().unwrap_or_else(|e| e.into_inner()).remove(tile_id); }
            }
            return;
        }
    }
    drop(times);
    let cli_path = crate::panorama_cli_path();
    let msg = if detached {
        format!("[panorama] {n} decision(s) needed in run {run_id}. Read them with: {cli_path} orch check --run-id {run_id} --decisions")
    } else {
        format!("[panorama] {n} new orchestration message(s) in run {run_id}. Read them with: {cli_path} orch check --run-id {run_id}")
    };
    if send_to_tile(tile_id, &msg).is_ok() {
        super::record_coordinator_wake(run_id, reason, n, detached);
    }
}

pub fn maybe_nudge_worker(tile_id: &str, cli_path: &str) {
    let now = Instant::now();
    {
        let mut map = nudge_times().lock().unwrap_or_else(|e| e.into_inner());
        if let Some(last) = map.get(tile_id) {
            if now.duration_since(*last) < Duration::from_secs(NUDGE_COOLDOWN_S) {
                return;
            }
        }
        map.insert(tile_id.to_string(), now);
    }
    let msg = format!("[panorama] new orchestration message(s). Read them with: {cli_path} orch check");
    let _ = send_to_tile(tile_id, &msg);
}

pub fn is_tile_waiting(tile_id: &str) -> bool {
    let session = sessions().lock().unwrap().get(tile_id).cloned();
    if let Some(s) = session {
        if let Some(state_str) = cached_agent_state(&s) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&state_str) {
                return v.get("lastStatus").and_then(|s| s.as_str()) == Some("waiting");
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orch::model::{Task, TaskStatus, Dispatch, DispatchStatus};
    use std::path::PathBuf;

    #[test]
    fn nudge_policy() {
        assert_eq!(nudge_decision(true, 1, None, false), NudgeDecision::Now);
        assert_eq!(nudge_decision(true, 1, Some(Duration::from_secs(10)), false), NudgeDecision::Defer(Duration::from_secs(20)));
        assert_eq!(nudge_decision(true, 1, Some(Duration::from_secs(30)), false), NudgeDecision::Now);
        assert_eq!(nudge_decision(false, 1, None, false), NudgeDecision::Skip);
        assert_eq!(nudge_decision(true, 0, None, false), NudgeDecision::Skip);
        assert_eq!(nudge_decision(true, 1, None, true), NudgeDecision::Skip);
    }

    #[tokio::test(start_paused = true)]
    async fn a_deferred_coordinator_nudge_does_not_record_a_wake() {
        use crate::orch::{state, unix_ms, WakeReason};

        crate::orch::tests::f12_test_home();
        let run_id = format!("r-deferred-wake-{}", unix_ms());
        let tile = format!("tile-{run_id}");
        let mut entry = crate::orch::tests::mk_run_entry(&run_id);
        entry.run.coordinator_tile = Some(tile.clone());
        state().lock().unwrap().runs.insert(run_id.clone(), entry);
        nudge_times().lock().unwrap().insert(tile.clone(), Instant::now());

        nudge_coordinator(&tile, &run_id, WakeReason::Messages(vec!["worker_done".into()]), 1, false, true);

        let entry = state().lock().unwrap().runs.remove(&run_id).unwrap();
        nudge_times().lock().unwrap().remove(&tile);
        assert!(entry.coordinator_wakes.is_empty());
        assert!(!crate::orch::run_events_path(&crate::panorama_dir().unwrap(), &run_id).exists());
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(NUDGE_COOLDOWN_S)).await;
        tokio::task::yield_now().await;
    }

    #[test]
    fn early_exit_needs_polls_and_min_wait() {
        let late = Duration::from_secs(EXITED_MIN_WAIT_S);
        assert!(exited_during_startup(EXITED_SHELL_POLLS, late));
        assert!(!exited_during_startup(EXITED_SHELL_POLLS - 1, late));
        assert!(!exited_during_startup(EXITED_SHELL_POLLS, Duration::from_secs(EXITED_MIN_WAIT_S - 1)));
        assert!(!exited_during_startup(0, Duration::from_secs(60)));
    }

    #[test]
    fn worker_settings_allow_rules() {
        let verify = vec!["bun x tsc --noEmit".to_string(), "cargo test:*".to_string()];
        let json = build_worker_settings(&verify);
        let allow: Vec<&str> = json["permissions"]["allow"].as_array().unwrap()
            .iter().filter_map(|v| v.as_str()).collect();
        assert!(allow.contains(&"Read(~/.claude/skills/**)"));
        assert!(allow.contains(&"Bash(bun x tsc --noEmit)"));
        assert!(allow.contains(&"PowerShell(cargo test:*)"));
        assert_eq!(allow.iter().filter(|r| **r == "Bash(cargo test:*)").count(), 1);
    }

    fn tmp_dir(suffix: &str) -> PathBuf {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("orch-worker-{suffix}-{ms}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dummy_task() -> Task {
        Task {
            id: "t1".into(),
            title: "Create hello.txt".into(),
            spec: "Create a file hello.txt containing 'hi'.".into(),
            acceptance: Some("hello.txt exists with 'hi'".into()),
            deps: vec![],
            profile: Some("claude".into()),
            escalation: vec![],
            verify: vec!["test -f hello.txt".into()],
            role: None,
            review: false,
            owner: None,
            owner_reason: None,
            status: TaskStatus::Dispatched,
            result: None,
            integrated_sha: None,
            attempts: 1,
            routing: 0, quality_floor: 0, quality_attempts: 0, operational_attempts: 0, pinned: false, last_failure: None, wait: None,
            created_at: 1_000_000,
            updated_at: 1_000_000,
        }
    }

    fn dummy_dispatch() -> Dispatch {
        Dispatch {
            id: "r1.t1.1".into(),
            task_id: "t1".into(),
            attempt: 1,
            retry_of: None,
            profile: Some("claude".into()),
            model: Some("claude-haiku-4-5".into()),
            tile_id: Some("tile123".into()),
            merge_branch: None,
            worktree: None,
            branch: None,
            status: DispatchStatus::Running,
            outcome: None,
            files_modified: vec![],
            summary: None, report: None,
            head_sha: None,
            started_at: 1_000_000,
            ended_at: None,
            last_heartbeat: None,
            verifications: vec![],
            cost_usd: None,
            cost_source: None,
            account: None, chain_index: None, trigger: None, decision: None, failure_class: None, by: None, start_ref: None,
            native_home: None, native_session: None, corrective_retry: false, retry_same_entry: false, operational_wait: false,
        }
    }

    #[test]
    fn native_resume_uses_the_provider_command_and_pastes_the_correction() {
        let dir = tmp_dir("resume-command");
        let mut profiles = crate::orch::profiles::load_profiles(&dir);
        let path = dir.join("brief.md");
        for profile in profiles.iter_mut().filter(|p| matches!(p.exe.as_str(), "claude" | "codex")) {
            profile.probe_permissions = false;
            let mut launch = WorkerLaunch {
                tile_id: "tile", cwd: dir.to_str().unwrap(), group: None, profile,
                model: Some("test-model"), extra_env: vec![], brief_path: &path,
                settings_path: None, resume_id: Some("native-id"),
            };
            let env = vec![("PANORAMA_DISPATCH_ID".into(), "r1.t1.2".into())];
            let command = worker_command(&launch, "/cli", &env);
            if profile.exe == "codex" {
                assert!(command.starts_with("codex resume "), "{command}");
                assert!(command.contains("PANORAMA_DISPATCH_ID"), "{command}");
                assert!(command.ends_with(" native-id"), "{command}");
            } else {
                assert!(command.ends_with(" --resume native-id"), "{command}");
            }
            assert!(!command.contains("Read the file"), "{command}");
            launch.resume_id = None;
            let fresh = worker_command(&launch, "/cli", &env);
            assert!(!fresh.contains("native-id"), "{fresh}");
            assert_eq!(fresh.contains("Read the file"), profile.exe == "codex");
        }
    }

    #[test]
    fn correction_contains_failed_check_and_report_command() {
        let mut previous = dummy_dispatch();
        previous.verifications.push(crate::orch::model::VerificationResult {
            command: "cargo test".into(), exit_code: Some(3), timed_out: false,
            tail: vec!["failed assertion".into()], head_sha: None, at: 5,
        });
        let message = corrective_message(&previous, "/cli");
        for detail in ["cargo test", "code 3", "failed assertion", "/cli orch send --type worker_done", "PANORAMA_DISPATCH_ID"] {
            assert!(message.contains(detail), "{message}");
        }
    }

    #[test]
    fn brief_contains_contract_and_cli_path() {
        let dir = tmp_dir("contract");
        let task = dummy_task();
        let dispatch = dummy_dispatch();
        let cli = "/usr/local/bin/panorama-cli";
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &[], cli_path: cli, cwd: "/tmp/work", merge_note: None, handoff: None, correction: None }).unwrap();
        let content = std::fs::read_to_string(&brief_path).unwrap();
        assert!(content.contains(cli), "cli path in brief");
        assert!(content.contains("worker_done"), "worker_done in brief");
        assert!(content.contains("For a failed outcome, start with Blocker:"));
        assert!(content.contains("Use --report-file for the full report"));
        assert!(content.contains("all findings with file:line evidence"));
        assert!(content.contains("file:line evidence"));
        assert!(content.contains("temporary file outside the worktree"));
        assert!(content.contains("PANORAMA_RUN_ID"), "env var reference in brief");
        assert!(content.contains("Create hello.txt"), "task title in brief");
        assert!(content.contains("/tmp/work"), "cwd in brief");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn brief_starts_with_instructions_and_preserves_spec() {
        let dir = tmp_dir("preamble");
        let mut task = dummy_task();
        task.spec = "First line.\n\n  Preserve spacing and `markdown`.\n".into();
        let dispatch = dummy_dispatch();
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &[], cli_path: "/cli", cwd: "/tmp", merge_note: None, handoff: None, correction: None }).unwrap();
        let content = std::fs::read_to_string(&brief_path).unwrap();
        assert!(content.starts_with("Read CLAUDE.md and AGENTS.md at the worktree root if they exist, and follow their conventions.\nIf those instructions conflict with the task spec below, the task spec wins.\n\n"));
        assert!(content.contains(&format!("## Spec\n\n{}\n\n", task.spec)));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn brief_includes_acceptance_and_verify() {
        let dir = tmp_dir("accept");
        let task = dummy_task();
        let dispatch = dummy_dispatch();
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &[], cli_path: "/cli", cwd: "/tmp", merge_note: None, handoff: None, correction: None }).unwrap();
        let content = std::fs::read_to_string(&brief_path).unwrap();
        assert!(content.contains("hello.txt exists"), "acceptance in brief");
        assert!(content.contains("test -f hello.txt"), "verify cmd in brief");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn brief_includes_deps_results() {
        let dir = tmp_dir("deps");
        let task = dummy_task();
        let dispatch = dummy_dispatch();
        let deps = vec![("t0".to_string(), Some("output: done".to_string()))];
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &deps, cli_path: "/cli", cwd: "/tmp", merge_note: None, handoff: None, correction: None }).unwrap();
        let content = std::fs::read_to_string(&brief_path).unwrap();
        assert!(content.contains("output: done"), "dep result in brief");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nudge_rate_limiting() {
        let tile = format!("tile-nudge-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        {
            let mut map = nudge_times().lock().unwrap();
            map.remove(&tile);
        }
        let now = Instant::now();

        {
            let mut map = nudge_times().lock().unwrap();
            map.insert(tile.clone(), now - Duration::from_secs(NUDGE_COOLDOWN_S + 1));
        }
        let old_elapsed = nudge_times().lock().unwrap()
            .get(&tile).map(|t| now.duration_since(*t)).unwrap();
        assert!(old_elapsed > Duration::from_secs(NUDGE_COOLDOWN_S), "old entry should be past cooldown");

        {
            let mut map = nudge_times().lock().unwrap();
            map.insert(tile.clone(), now);
        }
        let fresh_elapsed = nudge_times().lock().unwrap()
            .get(&tile).map(|t| Instant::now().duration_since(*t)).unwrap();
        assert!(fresh_elapsed < Duration::from_secs(NUDGE_COOLDOWN_S), "just nudged tile should be within cooldown");

        nudge_times().lock().unwrap().remove(&tile);
    }

    #[test]
    fn nudge_no_waiting_tile() {
        let tile = "tile-no-waiting-xyz";
        assert!(!is_tile_waiting(tile), "unknown tile not waiting");
    }
}

#[cfg(test)]
#[test]
fn detects_codex_trust_dialog() {
    let screen = "Folder access\n/worktree\nNote: You're in a subdirectory of a Git project. Trusting will apply to the repository root: /repo\nTrust this folder? Codex can read, edit, and run files here, subject to your permission settings.\n> 1. Trust and continue\n  2. Back to Agent Command Center\n  enter continue . esc back";
    assert!(codex_trust_dialog(screen));
    assert!(!codex_trust_dialog("Trust this folder?"));
    assert!(!codex_trust_dialog("Trust and continue"));
    assert!(!codex_trust_dialog("Ready for your prompt"));
}
