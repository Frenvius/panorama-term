use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::agents::launch::{self, AgentCommand};
use crate::agents::model::{Agent, Desired, LiveStatus};
use crate::agents::reducer::{Cmd, RESTART_MAX, RESTART_WINDOW_MS};
use crate::agents::{commit_cmd, lock, store, unix_ms};
use crate::orch::profiles::{find_profile, load_profiles, resolve_env, Profile};
use crate::orch::worker::{self, WORKER_COLS, WORKER_ROWS};

const BACKOFF_SECS: [u64; RESTART_MAX] = [5, 30, 120];
const SUPERVISION_TICK: Duration = Duration::from_secs(10);
const QUIET_TICK: Duration = Duration::from_secs(2);
const QUIET_IDLE_AFTER: Duration = Duration::from_secs(3);
const SESSION_GRACE: Duration = Duration::from_secs(60);
const RESUME_FAIL_WINDOW: Duration = Duration::from_secs(15);
const RESTORE_STAGGER: Duration = Duration::from_secs(2);
const SHELL_TICKS_EXITED: u8 = 2;
const CAPTURE_ATTEMPTS: u8 = 30;
const SESSION_LIST_TIMEOUT: Duration = Duration::from_secs(15);
const STATUS_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Watch {
    launched_at: Option<Instant>,
    resumed: bool,
    ever_live: bool,
    shell_ticks: u8,
}

pub struct QuietState {
    screen: String,
    changed_at: Instant,
    status: LiveStatus,
}

struct Capture {
    before: HashSet<String>,
    attempts: u8,
}

fn starting() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

fn pending_restart() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

fn watches() -> &'static Mutex<HashMap<String, Watch>> {
    static MAP: OnceLock<Mutex<HashMap<String, Watch>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn quiet_states() -> &'static Mutex<HashMap<String, QuietState>> {
    static MAP: OnceLock<Mutex<HashMap<String, QuietState>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn captures() -> &'static Mutex<HashMap<String, Capture>> {
    static MAP: OnceLock<Mutex<HashMap<String, Capture>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn guard<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_starting(id: &str) -> bool {
    guard(starting()).contains(id) || guard(pending_restart()).contains(id)
}

pub fn quiet_status(tile_id: &str) -> Option<LiveStatus> {
    guard(quiet_states()).get(tile_id).map(|q| q.status)
}

pub fn backoff(restarts_in_window: usize) -> Option<Duration> {
    BACKOFF_SECS.get(restarts_in_window).map(|s| Duration::from_secs(*s))
}

fn agent_snapshot(id: &str) -> Option<Agent> {
    lock().agents.get(id).cloned()
}

fn find_agent_profile(agent: &Agent) -> Option<(Profile, std::path::PathBuf)> {
    let base = crate::panorama_dir()?;
    let profile = find_profile(&load_profiles(&base), &agent.profile)?.clone();
    Some((profile, base))
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchError {
    ResumeFailed(String),
    Failed(String),
    Cancelled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FailureAction {
    Ignore,
    RetryFresh,
    Crash,
}

pub fn failure_action(err: &LaunchError, desired: Desired) -> FailureAction {
    match err {
        _ if desired != Desired::Running => FailureAction::Ignore,
        LaunchError::Cancelled => FailureAction::Ignore,
        LaunchError::ResumeFailed(_) => FailureAction::RetryFresh,
        LaunchError::Failed(_) => FailureAction::Crash,
    }
}

fn checkpoint(id: &str) -> Result<(), LaunchError> {
    match agent_snapshot(id) {
        Some(agent) if agent.desired == Desired::Running => Ok(()),
        _ => Err(LaunchError::Cancelled),
    }
}

fn desired_of(id: &str) -> Desired {
    agent_snapshot(id).map_or(Desired::Stopped, |a| a.desired)
}

#[derive(Debug, PartialEq, Eq)]
pub enum SessionPlan {
    KeepRunning,
    Recreate,
    Create,
}

pub fn session_plan(live: bool, cli_running: bool) -> SessionPlan {
    match (live, cli_running) {
        (true, true) => SessionPlan::KeepRunning,
        (true, false) => SessionPlan::Recreate,
        (false, _) => SessionPlan::Create,
    }
}

pub fn spawn_start(id: String) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(run_start(id));
    }
}

async fn run_start(id: String) {
    if !guard(starting()).insert(id.clone()) {
        return;
    }
    crate::ui::broadcast_agents_changed(&id);
    let tile = crate::agents::model::tile_id(&id);
    let mut result = launch_agent(&id, false).await;
    if let Err(err @ LaunchError::ResumeFailed(detail)) = &result {
        if failure_action(err, desired_of(&id)) == FailureAction::RetryFresh {
            record_resume_failure(&id, Some(detail));
            stop_session(&tile).await;
            result = launch_agent(&id, true).await;
        }
    }
    guard(starting()).remove(&id);
    if let Err(err) = result {
        match (failure_action(&err, desired_of(&id)), err) {
            (FailureAction::Ignore, _) => {
                if desired_of(&id) != Desired::Running {
                    stop_session(&tile).await;
                }
            }
            (_, LaunchError::Failed(detail) | LaunchError::ResumeFailed(detail)) => {
                stop_session(&tile).await;
                on_agent_gone(&id, "shell_returned", Some(&detail)).await;
            }
            (_, LaunchError::Cancelled) => {}
        }
    }
    crate::ui::broadcast_agents_changed(&id);
}

fn record_resume_failure(id: &str, detail: Option<&str>) {
    let at = unix_ms();
    let detail = detail.map(str::to_string);
    let _ = commit_cmd(id, Cmd::Exit { id: id.into(), at, reason: "resume_failed".into(), code: None, detail });
    let _ = commit_cmd(id, Cmd::SessionClear { id: id.into(), at, reason: "resume_failed".into() });
}

fn write_claude_settings(base: &Path, agent_id: &str) -> Option<String> {
    let path = store::agent_dir(base, agent_id).join("claude-settings.json");
    let json = worker::build_worker_settings(&[], false);
    let text = serde_json::to_string_pretty(&json).ok()?;
    store::ensure_agent_dir(base, agent_id).ok()?;
    std::fs::write(&path, text).ok()?;
    Some(crate::path_string(&path))
}

fn cli_running(tile_id: &str) -> bool {
    crate::lookup_live(tile_id).is_some_and(|s| crate::foreground_name_for(&s)
        .is_some_and(|fg| fg != crate::shell_name_for(&s)))
}

async fn launch_agent(id: &str, force_fresh: bool) -> Result<(), LaunchError> {
    let Some(agent) = agent_snapshot(id) else { return Ok(()) };
    if agent.desired != Desired::Running {
        return Ok(());
    }
    let tile = agent.tile_id();
    match session_plan(crate::lookup_live(&tile).is_some(), cli_running(&tile)) {
        SessionPlan::KeepRunning => return Ok(()),
        SessionPlan::Recreate => { stop_session(&tile).await; }
        SessionPlan::Create => {}
    }
    let Some((profile, base)) = find_agent_profile(&agent) else {
        return Err(LaunchError::Failed(format!("unknown profile {}", agent.profile)));
    };
    let mut env = resolve_env(&profile.env, Some(&base));
    let home = worker::pin_native_home(&profile.exe, &tile, &mut env);
    let resume_id = if force_fresh { None } else {
        launch::resume_id_for(&agent, &profile, launch::native_transcript_exists(home.as_deref().map(Path::new)))
            .map(str::to_string)
    };
    env.push(("PANORAMA_AGENT_ID".into(), agent.id.clone()));
    env.push(("PANORAMA_AGENT_NAME".into(), agent.name.clone()));
    let cli = crate::panorama_cli_path();
    let mut pinned = env.clone();
    pinned.push(("PANORAMA_CLI".into(), cli.clone()));
    pinned.push(("PANORAMA_SIDECAR_PORT".into(), crate::port().to_string()));
    pinned.push(("PANORAMA_TILE_ID".into(), tile.clone()));
    let settings_path = (profile.exe == "claude").then(|| write_claude_settings(&base, id)).flatten();
    let command = launch::agent_command(AgentCommand {
        agent: &agent, profile: &profile, cli: &cli, pinned_env: &pinned,
        settings_path: settings_path.as_deref(), resume_id: resume_id.as_deref(),
    });

    if resume_id.is_none() && needs_list_capture(&profile.exe) {
        let before = list_sessions(&profile.exe, &agent.cwd).await.unwrap_or_default();
        guard(captures()).insert(id.to_string(), Capture { before, attempts: 0 });
        checkpoint(id)?;
    }

    let params = crate::Params {
        tile_id: tile.clone(),
        cols: WORKER_COLS,
        rows: WORKER_ROWS,
        cwd: Some(agent.cwd.clone()),
        shell: None,
        target: None,
        elevated: false,
        idle_compact: true,
        spawn_cmd: None,
        run_kind: "agent".into(),
        attach_only: false,
        watch_only: false,
        remote: false,
        extra_env: env,
    };
    crate::get_or_create(&params).await.map_err(LaunchError::Failed)?;
    checkpoint(id)?;
    tokio::time::sleep(Duration::from_millis(worker::SHELL_SETTLE_MS)).await;
    checkpoint(id)?;
    if profile.exe == "agy" {
        crate::orch::agy::ensure_trusted_workspace(&agent.cwd);
    }
    if resume_id.is_some() && profile.exe == "claude" {
        crate::clear_binding_session(&tile);
    }
    guard(watches()).insert(id.to_string(), Watch {
        launched_at: Some(Instant::now()), resumed: resume_id.is_some(), ever_live: true, shell_ticks: 0,
    });
    crate::send_to_tile(&tile, &command).map_err(LaunchError::Failed)?;
    if let Err(e) = worker::wait_for_agent_ready(&tile, &profile, resume_id.as_deref()).await {
        return Err(match resume_id {
            Some(_) => LaunchError::ResumeFailed(e),
            None => LaunchError::Failed(e),
        });
    }
    checkpoint(id)?;
    if resume_id.is_none() {
        crate::agents::context::after_fresh_start(&agent, &profile).await;
    }
    try_capture_session(id).await;
    Ok(())
}

fn needs_list_capture(exe: &str) -> bool {
    matches!(exe, "kimi" | "opencode")
}

fn session_list_command(exe: &str, cwd: &str) -> Option<(String, Vec<String>)> {
    let args: Vec<&str> = match exe {
        "kimi" => vec!["session", "list", "--json", "--cwd", cwd],
        "opencode" => vec!["session", "list", "--format", "json"],
        _ => return None,
    };
    Some((exe.to_string(), args.into_iter().map(str::to_string).collect()))
}

pub fn parse_session_ids(text: &str) -> Option<HashSet<String>> {
    let start = text.find('[')?;
    let value: serde_json::Value = serde_json::from_str(text[start..].trim()).ok()?;
    Some(value.as_array()?.iter()
        .filter_map(|s| s["id"].as_str().or_else(|| s["sessionID"].as_str()))
        .map(str::to_string)
        .collect())
}

pub fn new_session_id(before: &HashSet<String>, after: &HashSet<String>) -> Option<String> {
    let mut fresh = after.difference(before);
    let first = fresh.next()?;
    fresh.next().is_none().then(|| first.clone())
}

async fn list_sessions(exe: &str, cwd: &str) -> Option<HashSet<String>> {
    let (program, args) = session_list_command(exe, cwd)?;
    let cwd = cwd.to_string();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let mut cmd = crate::hidden_command(&program);
        cmd.args(&args).current_dir(&cwd);
        if let Some(path) = crate::orch::cli_env::cli_path() {
            cmd.env("PATH", path);
        }
        let _ = tx.send(cmd.output().ok());
    });
    let output = tokio::time::timeout(SESSION_LIST_TIMEOUT, rx).await.ok()?.ok()??;
    parse_session_ids(&String::from_utf8_lossy(&output.stdout))
}

async fn try_capture_session(id: &str) {
    let Some(agent) = agent_snapshot(id) else { return };
    if agent.native_session.is_some() {
        guard(captures()).remove(id);
        return;
    }
    let Some(profile) = find_agent_profile(&agent).map(|(p, _)| p) else { return };
    let before = {
        let mut map = guard(captures());
        let Some(capture) = map.get_mut(id) else { return };
        if capture.attempts >= CAPTURE_ATTEMPTS {
            return;
        }
        capture.attempts += 1;
        capture.before.clone()
    };
    let Some(after) = list_sessions(&profile.exe, &agent.cwd).await else { return };
    if let Some(session_id) = new_session_id(&before, &after) {
        guard(captures()).remove(id);
        bind_session(id, &profile.exe, &session_id);
    }
}

pub fn bind_session(id: &str, cli: &str, session_id: &str) {
    if !worker::valid_session_id(session_id) {
        return;
    }
    let cmd = Cmd::BindSession { id: id.into(), agent: cli.into(), session_id: session_id.into(), at: unix_ms() };
    if let Err(e) = commit_cmd(id, cmd) {
        eprintln!("[agents] {id}: session not bound: {}", e.message);
    }
}

pub async fn stop_session(tile_id: &str) -> bool {
    let session = crate::sessions().lock().unwrap().get(tile_id).cloned();
    let Some(session) = session else { return true };
    if !session.exited.load(std::sync::atomic::Ordering::Acquire) {
        crate::kill_session(&session);
    }
    let deadline = Instant::now() + crate::RUN_KILL_CONFIRM;
    while !session.exited.load(std::sync::atomic::Ordering::Acquire) {
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    true
}

pub fn forget(id: &str, tile_id: &str) {
    guard(watches()).remove(id);
    guard(captures()).remove(id);
    guard(pending_restart()).remove(id);
    guard(quiet_states()).remove(tile_id);
}

async fn on_agent_gone(id: &str, reason: &str, detail: Option<&str>) {
    let Some(agent) = agent_snapshot(id) else { return };
    if agent.desired != Desired::Running {
        return;
    }
    let at = unix_ms();
    stop_session(&agent.tile_id()).await;
    let watch = guard(watches()).remove(id).unwrap_or_default();
    if resume_failed(&watch, Instant::now()) {
        record_resume_failure(id, detail);
    } else {
        let detail = detail.map(str::to_string);
        let _ = commit_cmd(id, Cmd::Exit { id: id.into(), at, reason: reason.into(), code: None, detail });
    }
    match crash_action(&agent.restarts, at) {
        CrashAction::RestartAfter(delay) if commit_cmd(id, Cmd::Restart { id: id.into(), at }).is_ok() => {
            schedule_restart(id.to_string(), delay)
        }
        _ => mark_crashed(id).await,
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum CrashAction {
    RestartAfter(Duration),
    Crash,
}

pub fn crash_action(restarts: &[u64], at: u64) -> CrashAction {
    let in_window = restarts.iter().filter(|t| at.saturating_sub(**t) < RESTART_WINDOW_MS).count();
    backoff(in_window).map_or(CrashAction::Crash, CrashAction::RestartAfter)
}

pub fn resume_failed(watch: &Watch, now: Instant) -> bool {
    watch.resumed && watch.launched_at.is_some_and(|t| now.duration_since(t) < RESUME_FAIL_WINDOW)
}

async fn mark_crashed(id: &str) {
    let cmd = Cmd::Stop { id: id.into(), at: unix_ms(), by: "brain".into(), force: true, crashed: true };
    let _ = commit_cmd(id, cmd);
    let tile = crate::agents::model::tile_id(id);
    stop_session(&tile).await;
    forget(id, &tile);
}

fn schedule_restart(id: String, delay: Duration) {
    if !guard(pending_restart()).insert(id.clone()) {
        return;
    }
    crate::ui::broadcast_agents_changed(&id);
    tokio::spawn(async move {
        tokio::time::sleep(delay).await;
        guard(pending_restart()).remove(&id);
        run_start(id).await;
    });
}

pub fn restores_on_boot(agent: &Agent) -> bool {
    agent.desired == Desired::Running && agent.auto_start
}

pub fn restore_all() {
    let ids: Vec<String> = lock().agents.values()
        .filter(|a| restores_on_boot(a))
        .map(|a| a.id.clone())
        .collect();
    let Ok(handle) = tokio::runtime::Handle::try_current() else { return };
    handle.spawn(async move {
        for id in ids {
            let tile = crate::agents::model::tile_id(&id);
            if crate::lookup_live(&tile).is_some() {
                guard(watches()).insert(id.clone(), Watch { ever_live: true, ..Default::default() });
                continue;
            }
            spawn_start(id);
            tokio::time::sleep(RESTORE_STAGGER).await;
        }
    });
    handle.spawn(supervision_loop());
    handle.spawn(quiet_loop());
}

#[derive(Debug, PartialEq, Eq)]
pub enum Finding {
    SessionGone,
    ShellReturned,
}

pub fn assess(live: bool, at_shell: bool, watch: &mut Watch, since_start: Option<Duration>) -> Option<Finding> {
    if !live {
        let in_grace = !watch.ever_live && since_start.is_some_and(|d| d < SESSION_GRACE);
        return (!in_grace).then_some(Finding::SessionGone);
    }
    watch.ever_live = true;
    watch.shell_ticks = if at_shell { watch.shell_ticks.saturating_add(1) } else { 0 };
    (watch.shell_ticks >= SHELL_TICKS_EXITED).then_some(Finding::ShellReturned)
}

async fn supervision_loop() {
    let mut interval = tokio::time::interval(SUPERVISION_TICK);
    loop {
        interval.tick().await;
        supervision_tick().await;
    }
}

async fn supervision_tick() {
    let running: Vec<Agent> = lock().agents.values().filter(|a| a.desired == Desired::Running).cloned().collect();
    for agent in running {
        if is_starting(&agent.id) {
            continue;
        }
        let tile = agent.tile_id();
        let live = crate::lookup_live(&tile).is_some();
        let at_shell = live && worker::agent_back_at_shell(&tile);
        let since_start = agent.last_started_at.map(|t| Duration::from_millis(unix_ms().saturating_sub(t)));
        let finding = {
            let mut map = guard(watches());
            let watch = map.entry(agent.id.clone()).or_default();
            assess(live, at_shell, watch, since_start)
        };
        match finding {
            Some(Finding::SessionGone) => on_agent_gone(&agent.id, "session_gone", None).await,
            Some(Finding::ShellReturned) => on_agent_gone(&agent.id, "shell_returned", None).await,
            None if agent.native_session.is_none() => try_capture_session(&agent.id).await,
            None => {}
        }
    }
}

fn quiet_targets() -> Vec<(String, String, Option<String>)> {
    let Some(base) = crate::panorama_dir() else { return vec![] };
    let profiles = load_profiles(&base);
    lock().agents.values()
        .filter(|a| a.desired == Desired::Running)
        .filter_map(|a| {
            let profile = find_profile(&profiles, &a.profile)?;
            (!launch::has_status_hook(&profile.exe)).then(|| (a.id.clone(), a.tile_id(), profile.ready_text.clone()))
        })
        .collect()
}

pub fn quiet_step(previous: Option<&QuietState>, screen: &str, now: Instant, ready_visible: bool) -> QuietState {
    let changed_at = match previous {
        Some(p) if p.screen == screen => p.changed_at,
        _ => now,
    };
    let status = if now.duration_since(changed_at) >= QUIET_IDLE_AFTER && ready_visible {
        LiveStatus::Idle
    } else {
        LiveStatus::Busy
    };
    QuietState { screen: screen.to_string(), changed_at, status }
}

async fn quiet_loop() {
    let mut interval = tokio::time::interval(QUIET_TICK);
    loop {
        interval.tick().await;
        if !crate::ui::has_subscribers() {
            continue;
        }
        for (id, tile, ready_text) in quiet_targets() {
            if crate::lookup_live(&tile).is_none() || is_starting(&id) {
                guard(quiet_states()).remove(&tile);
                continue;
            }
            let screen = worker::capture_worker_screen(&tile, 40);
            let ready_visible = ready_text.as_deref().is_none_or(|t| screen.contains(t));
            let changed = {
                let mut map = guard(quiet_states());
                let next = quiet_step(map.get(&tile), &screen, Instant::now(), ready_visible);
                let changed = map.get(&tile).is_none_or(|p| p.status != next.status);
                map.insert(tile.clone(), next);
                changed
            };
            if changed {
                crate::ui::broadcast_agents_changed(&id);
            }
        }
    }
}

pub fn status_touched(agent_id: &str) {
    if agent_snapshot(agent_id).is_none() {
        return;
    }
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let pending = PENDING.get_or_init(Default::default);
    if !guard(pending).insert(agent_id.to_string()) {
        return;
    }
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        guard(pending).remove(agent_id);
        crate::ui::broadcast_agents_changed(agent_id);
        return;
    };
    let id = agent_id.to_string();
    handle.spawn(async move {
        tokio::time::sleep(STATUS_DEBOUNCE).await;
        guard(pending).remove(&id);
        crate::ui::broadcast_agents_changed(&id);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_follows_the_schedule_then_stops() {
        assert_eq!(backoff(0), Some(Duration::from_secs(5)));
        assert_eq!(backoff(1), Some(Duration::from_secs(30)));
        assert_eq!(backoff(2), Some(Duration::from_secs(120)));
        assert_eq!(backoff(3), None);
    }

    #[test]
    fn a_stop_during_start_never_records_a_failure() {
        let resume = LaunchError::ResumeFailed("killed".into());
        let failed = LaunchError::Failed("killed".into());
        assert_eq!(failure_action(&resume, Desired::Stopped), FailureAction::Ignore);
        assert_eq!(failure_action(&failed, Desired::Stopped), FailureAction::Ignore);
        assert_eq!(failure_action(&LaunchError::Cancelled, Desired::Running), FailureAction::Ignore);
        assert_eq!(failure_action(&resume, Desired::Running), FailureAction::RetryFresh);
        assert_eq!(failure_action(&failed, Desired::Running), FailureAction::Crash);
    }

    #[test]
    fn launch_checkpoint_cancels_once_the_agent_is_stopped() {
        let id = format!("ackpt{}", unix_ms() % 1_000_000);
        let mut agent = crate::agents::reducer::tests::agent(&id, "ckpt");
        agent.desired = Desired::Running;
        lock().agents.insert(id.clone(), agent);
        let running = checkpoint(&id);
        lock().agents.get_mut(&id).unwrap().desired = Desired::Stopped;
        let stopped = checkpoint(&id);
        lock().agents.remove(&id);
        let gone = checkpoint(&id);
        assert_eq!(running, Ok(()));
        assert_eq!(stopped, Err(LaunchError::Cancelled));
        assert_eq!(gone, Err(LaunchError::Cancelled));
    }

    #[test]
    fn a_plain_shell_under_the_agent_tile_is_recreated() {
        assert_eq!(session_plan(true, true), SessionPlan::KeepRunning);
        assert_eq!(session_plan(true, false), SessionPlan::Recreate);
        assert_eq!(session_plan(false, false), SessionPlan::Create);
    }

    #[test]
    fn restore_on_boot_needs_running_and_auto_start() {
        let mut agent = crate::agents::reducer::tests::agent("a1", "r");
        agent.desired = Desired::Running;
        agent.auto_start = true;
        assert!(restores_on_boot(&agent));
        agent.auto_start = false;
        assert!(!restores_on_boot(&agent));
        agent.auto_start = true;
        agent.desired = Desired::Stopped;
        assert!(!restores_on_boot(&agent));
    }

    #[test]
    fn crash_policy_backs_off_then_crashes() {
        let t = 10 * RESTART_WINDOW_MS;
        let minute = 60_000;
        assert_eq!(crash_action(&[], t), CrashAction::RestartAfter(Duration::from_secs(5)));
        assert_eq!(crash_action(&[t - minute], t), CrashAction::RestartAfter(Duration::from_secs(30)));
        assert_eq!(crash_action(&[t - 2 * minute, t - minute], t), CrashAction::RestartAfter(Duration::from_secs(120)));
        assert_eq!(crash_action(&[t - 3 * minute, t - 2 * minute, t - minute], t), CrashAction::Crash);
        assert_eq!(crash_action(&[t - RESTART_WINDOW_MS, t - 2 * minute, t - minute], t), CrashAction::RestartAfter(Duration::from_secs(120)));
    }

    #[test]
    fn early_exit_after_resume_counts_as_resume_failure() {
        let t0 = Instant::now();
        let watch = Watch { launched_at: Some(t0), resumed: true, ever_live: true, shell_ticks: 0 };
        assert!(resume_failed(&watch, t0 + Duration::from_secs(10)));
        assert!(!resume_failed(&watch, t0 + Duration::from_secs(16)));
        let fresh = Watch { resumed: false, ..watch };
        assert!(!resume_failed(&fresh, t0 + Duration::from_secs(1)));
    }

    #[test]
    fn supervision_grace_and_shell_ticks() {
        let mut watch = Watch::default();
        assert_eq!(assess(false, false, &mut watch, Some(Duration::from_secs(10))), None);
        assert_eq!(assess(false, false, &mut watch, Some(Duration::from_secs(61))), Some(Finding::SessionGone));
        let mut watch = Watch::default();
        assert_eq!(assess(true, true, &mut watch, None), None);
        assert_eq!(assess(true, false, &mut watch, None), None);
        assert_eq!(assess(true, true, &mut watch, None), None);
        assert_eq!(assess(true, true, &mut watch, None), Some(Finding::ShellReturned));
        assert_eq!(assess(false, false, &mut watch, Some(Duration::from_secs(1))), Some(Finding::SessionGone));
    }

    #[test]
    fn quiet_detector_needs_three_still_seconds_and_ready_text() {
        let t0 = Instant::now();
        let first = quiet_step(None, "a", t0, true);
        assert_eq!(first.status, LiveStatus::Busy);
        let still = quiet_step(Some(&first), "a", t0 + Duration::from_secs(3), true);
        assert_eq!(still.status, LiveStatus::Idle);
        let no_ready = quiet_step(Some(&first), "a", t0 + Duration::from_secs(4), false);
        assert_eq!(no_ready.status, LiveStatus::Busy);
        let moved = quiet_step(Some(&still), "b", t0 + Duration::from_secs(5), true);
        assert_eq!(moved.status, LiveStatus::Busy);
        assert_eq!(moved.changed_at, t0 + Duration::from_secs(5));
    }

    #[test]
    fn kimi_session_list_fixture_parses() {
        let text = include_str!("fixtures/kimi-session-list.json");
        let ids = parse_session_ids(text).unwrap();
        assert!(ids.contains("session_260442e5-eb8a-464d-8dc3-90de3673712c"));
        assert_eq!(ids.len(), 2);
    }

    #[test]
    fn opencode_style_list_parses_and_empty_output_is_none() {
        let ids = parse_session_ids("[{\"id\":\"ses_1\",\"title\":\"x\"},{\"sessionID\":\"ses_2\"}]").unwrap();
        assert_eq!(ids.len(), 2);
        assert!(parse_session_ids("").is_none());
        assert!(parse_session_ids("warning: something\n[]").unwrap().is_empty());
    }

    #[test]
    fn diff_picks_only_a_single_new_id() {
        let set = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
        assert_eq!(new_session_id(&set(&["a"]), &set(&["a", "b"])), Some("b".into()));
        assert_eq!(new_session_id(&set(&["a"]), &set(&["a"])), None);
        assert_eq!(new_session_id(&set(&["a"]), &set(&["a", "b", "c"])), None);
        assert_eq!(new_session_id(&set(&[]), &set(&["x"])), Some("x".into()));
    }
}
