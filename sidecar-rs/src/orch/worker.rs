use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::orch::model::{Task, Dispatch};
use crate::orch::profiles::{Profile, ReadySignal, build_command_line, tokens_to_command};
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
}

pub struct WorkerLaunch<'a> {
    pub tile_id: &'a str,
    pub cwd: &'a str,
    pub group: TileGroup<'a>,
    pub profile: &'a Profile,
    pub model: Option<&'a str>,
    pub extra_env: Vec<(String, String)>,
    pub brief_path: &'a Path,
}

pub fn write_brief(input: BriefInput) -> Result<PathBuf, String> {
    let BriefInput { base, run_id, dispatch_id, task, dispatch, deps_results, cli_path, cwd } = input;
    let dispatch_dir = base
        .join("orch")
        .join("runs")
        .join(run_id)
        .join("dispatch")
        .join(dispatch_id);
    std::fs::create_dir_all(&dispatch_dir)
        .map_err(|e| format!("create dispatch dir: {e}"))?;

    let brief_path = dispatch_dir.join("brief.md");

    let mut content = format!("# Task: {}\n\n", task.title);
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

    content.push_str(&format!("## Working directory\n\n`{cwd}`\n\n"));

    let attempt = dispatch.attempt;
    if attempt > 1 {
        content.push_str(&format!("## Attempt\n\nThis is attempt {attempt}.\n\n"));
    }

    content.push_str("## Contract\n\n");
    content.push_str("`PANORAMA_RUN_ID` and `PANORAMA_DISPATCH_ID` are already set in your environment; no ids are needed in commands.\n\n");
    content.push_str(&format!(
        "Report exactly once when finished:\n```\n{cli_path} orch send --type worker_done --outcome succeeded|failed --summary \"...\" [--file path ...]\n```\n\n"
    ));
    content.push_str(&format!(
        "Blocking question:\n```\n{cli_path} orch send --type question --body \"...\"\n{cli_path} orch check --wait --timeout-ms 600000\n```\n\n"
    ));
    content.push_str(&format!(
        "Optional progress:\n```\n{cli_path} orch send --type progress --body \"...\"\n```\n\n"
    ));
    content.push_str("Leave your changes in the working directory; Panorama commits them when you report. Do not run git commit.\n");
    content.push_str("Do not change files outside the working directory. Do not push.\n");

    std::fs::write(&brief_path, &content)
        .map_err(|e| format!("write brief: {e}"))?;

    Ok(brief_path)
}

pub async fn spawn_worker(launch: WorkerLaunch<'_>) -> Result<(), String> {
    let WorkerLaunch { tile_id, cwd, group, profile, model, extra_env, brief_path } = launch;
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

    broadcast_grouped_tile_open(tile_id, cwd, group);

    tokio::time::sleep(Duration::from_millis(SHELL_SETTLE_MS)).await;

    let cli = crate::panorama_cli_path();
    let tokens: Vec<String> = build_command_line(profile, model)
        .into_iter()
        .map(|t| t.replace(CLI_PLACEHOLDER, &cli))
        .collect();
    send_to_tile(tile_id, &tokens_to_command(&tokens))?;

    wait_for_agent_ready(tile_id, profile).await?;

    let brief = std::fs::read_to_string(brief_path).map_err(|e| format!("read brief: {e}"))?;
    send_to_tile(tile_id, &brief)
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

async fn wait_for_agent_ready(tile_id: &str, profile: &Profile) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(AGENT_READY_IDLE_TIMEOUT_S);
    let launch_time = Instant::now();
    let mut last_activity = Instant::now();
    let mut last_snapshot_len = 0usize;
    let mut answer_sends = vec![0u8; profile.startup_answers.len()];

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

        if quiet_duration >= Duration::from_millis(STARTUP_ANSWER_QUIET_MS) {
            let screen = capture_worker_screen(tile_id, 40);
            let pending = profile.startup_answers.iter().zip(answer_sends.iter_mut())
                .find(|(answer, sends)| **sends < STARTUP_ANSWER_MAX_SENDS && screen.contains(&answer.contains));
            if let Some((answer, sends)) = pending {
                *sends += 1;
                if let Some(s) = lookup_live(tile_id) {
                    send_keys(&s, &answer.send).await;
                }
                last_activity = Instant::now();
                tokio::time::sleep(Duration::from_millis(300)).await;
                continue;
            }
        }

        if let Some(s) = lookup_live(tile_id) {
            if let Some(state_str) = cached_agent_state(&s) {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&state_str) {
                    if v.get("lastStatus").and_then(|s| s.as_str()) == Some("idle") {
                        return Ok(());
                    }
                }
            }
        }

        let since_launch = Instant::now().duration_since(launch_time);
        if profile.ready_signal == ReadySignal::Quiet
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

pub fn maybe_nudge_tile(tile_id: &str, run_id: &str, n: usize, cli_path: &str) {
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
    let msg = format!(
        "[panorama] {n} new orchestration message(s) in run {run_id}. Read them with: {cli_path} orch check --run-id {run_id}"
    );
    let _ = send_to_tile(tile_id, &msg);
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
            review: false,
            status: TaskStatus::Dispatched,
            result: None,
            integrated_sha: None,
            attempts: 1,
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
            worktree: None,
            branch: None,
            status: DispatchStatus::Running,
            outcome: None,
            files_modified: vec![],
            summary: None,
            head_sha: None,
            started_at: 1_000_000,
            ended_at: None,
            last_heartbeat: None,
            verifications: vec![],
            cost_usd: None,
            cost_source: None,
        }
    }

    #[test]
    fn brief_contains_contract_and_cli_path() {
        let dir = tmp_dir("contract");
        let task = dummy_task();
        let dispatch = dummy_dispatch();
        let cli = "/usr/local/bin/panorama-cli";
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &[], cli_path: cli, cwd: "/tmp/work" }).unwrap();
        let content = std::fs::read_to_string(&brief_path).unwrap();
        assert!(content.contains(cli), "cli path in brief");
        assert!(content.contains("worker_done"), "worker_done in brief");
        assert!(content.contains("PANORAMA_RUN_ID"), "env var reference in brief");
        assert!(content.contains("Create hello.txt"), "task title in brief");
        assert!(content.contains("/tmp/work"), "cwd in brief");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn brief_includes_acceptance_and_verify() {
        let dir = tmp_dir("accept");
        let task = dummy_task();
        let dispatch = dummy_dispatch();
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &[], cli_path: "/cli", cwd: "/tmp" }).unwrap();
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
        let brief_path = write_brief(BriefInput { base: &dir, run_id: "r1", dispatch_id: "r1.t1.1", task: &task, dispatch: &dispatch, deps_results: &deps, cli_path: "/cli", cwd: "/tmp" }).unwrap();
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
