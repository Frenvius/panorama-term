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

#[derive(Debug, PartialEq, Eq)]
pub enum FinishAction {
    StopSession,
    Respawn,
    Crash(String),
}

pub fn finish_action(err: LaunchError, desired: Desired) -> FinishAction {
    match err {
        _ if desired != Desired::Running => FinishAction::StopSession,
        LaunchError::Cancelled => FinishAction::Respawn,
        LaunchError::Failed(detail) | LaunchError::ResumeFailed(detail) => FinishAction::Crash(detail),
    }
}

pub fn spawn_start(id: String) {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(run_start(id));
    }
}

trait StartBackend {
    fn live(&self, tile: &str) -> bool;
    async fn cli_running(&self, tile: &str) -> bool;
    async fn stop_session(&self, tile: &str) -> bool;
    async fn list_sessions(&self, exe: &str, cwd: &str) -> Option<HashSet<String>>;
    async fn ensure_session(&self, params: &crate::Params) -> Result<(), String>;
    async fn settle(&self);
    fn send(&self, tile: &str, text: &str) -> Result<(), String>;
    async fn wait_ready(&self, tile: &str, profile: &Profile, resume_id: Option<&str>) -> Result<(), String>;
    async fn after_fresh_start(&self, agent: &Agent, profile: &Profile);
    async fn capture_session(&self, id: &str);
    fn respawn(&self, id: &str);
    fn ready(&self, id: &str);
}

struct LiveBackend;

const FOREGROUND_RECHECK: Duration = Duration::from_millis(300);

impl StartBackend for LiveBackend {
    fn live(&self, tile: &str) -> bool { crate::lookup_live(tile).is_some() }
    async fn cli_running(&self, tile: &str) -> bool {
        if cli_running(tile) {
            return true;
        }
        tokio::time::sleep(FOREGROUND_RECHECK).await;
        cli_running(tile)
    }
    async fn stop_session(&self, tile: &str) -> bool { stop_session(tile).await }
    async fn list_sessions(&self, exe: &str, cwd: &str) -> Option<HashSet<String>> { list_sessions(exe, cwd).await }
    async fn ensure_session(&self, params: &crate::Params) -> Result<(), String> { crate::get_or_create(params).await.map(|_| ()) }
    async fn settle(&self) { tokio::time::sleep(Duration::from_millis(worker::SHELL_SETTLE_MS)).await }
    fn send(&self, tile: &str, text: &str) -> Result<(), String> { crate::send_to_tile(tile, text) }
    async fn wait_ready(&self, tile: &str, profile: &Profile, resume_id: Option<&str>) -> Result<(), String> {
        worker::wait_for_agent_ready(tile, profile, resume_id).await
    }
    async fn after_fresh_start(&self, agent: &Agent, profile: &Profile) {
        crate::agents::context::after_fresh_start(agent, profile).await
    }
    async fn capture_session(&self, id: &str) { try_capture_session(id).await }
    fn respawn(&self, id: &str) { spawn_start(id.to_string()) }
    fn ready(&self, id: &str) { crate::agents::delivery::kick(&crate::agents::mail::Party::Agent { id: id.to_string() }) }
}

async fn run_start(id: String) {
    run_start_with(&LiveBackend, id).await
}

async fn run_start_with<B: StartBackend>(backend: &B, id: String) {
    if !guard(starting()).insert(id.clone()) {
        return;
    }
    crate::ui::broadcast_agents_changed(&id);
    let tile = crate::agents::model::tile_id(&id);
    let mut result = launch_agent(backend, &id, false).await;
    if let Err(err @ LaunchError::ResumeFailed(detail)) = &result {
        if failure_action(err, desired_of(&id)) == FailureAction::RetryFresh {
            record_resume_failure(&id, Some(detail));
            backend.stop_session(&tile).await;
            result = launch_agent(backend, &id, true).await;
        }
    }
    guard(starting()).remove(&id);
    if let Err(err) = result {
        match finish_action(err, desired_of(&id)) {
            FinishAction::StopSession => { backend.stop_session(&tile).await; }
            FinishAction::Respawn => backend.respawn(&id),
            FinishAction::Crash(detail) => {
                backend.stop_session(&tile).await;
                on_agent_gone(&id, "shell_returned", Some(&detail)).await;
            }
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

async fn launch_agent<B: StartBackend>(backend: &B, id: &str, force_fresh: bool) -> Result<(), LaunchError> {
    let Some(agent) = agent_snapshot(id) else { return Ok(()) };
    if agent.desired != Desired::Running {
        return Ok(());
    }
    let tile = agent.tile_id();
    let live = backend.live(&tile);
    match session_plan(live, live && backend.cli_running(&tile).await) {
        SessionPlan::KeepRunning => return Ok(()),
        SessionPlan::Recreate => { backend.stop_session(&tile).await; }
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
        let before = backend.list_sessions(&profile.exe, &agent.cwd).await.unwrap_or_default();
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
    backend.ensure_session(&params).await.map_err(LaunchError::Failed)?;
    checkpoint(id)?;
    backend.settle().await;
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
    backend.send(&tile, &command).map_err(LaunchError::Failed)?;
    if let Err(e) = backend.wait_ready(&tile, &profile, resume_id.as_deref()).await {
        return Err(match resume_id {
            Some(_) => LaunchError::ResumeFailed(e),
            None => LaunchError::Failed(e),
        });
    }
    checkpoint(id)?;
    if resume_id.is_none() {
        backend.after_fresh_start(&agent, &profile).await;
    }
    backend.capture_session(id).await;
    backend.ready(id);
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
    remember_runtime(handle.clone());
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

fn quiet_targets() -> Vec<(String, String, Option<String>, bool)> {
    let Some(base) = crate::panorama_dir() else { return vec![] };
    let profiles = load_profiles(&base);
    lock().agents.values()
        .filter(|a| a.desired == Desired::Running)
        .filter_map(|a| {
            let profile = find_profile(&profiles, &a.profile)?;
            Some((a.id.clone(), a.tile_id(), profile.ready_text.clone(), launch::has_status_hook(&profile.exe)))
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
        let ui = crate::ui::has_subscribers();
        for (id, tile, ready_text, hook) in quiet_targets() {
            let mail = quiet_mail(&id);
            if !quiet_wanted(hook, ui, mail, !hook_status_known(&tile)) {
                continue;
            }
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
                status_settled(&id);
            }
        }
        for (tile, ready_text) in plain_quiet_targets() {
            if crate::lookup_live(&tile).is_none() {
                guard(quiet_states()).remove(&tile);
                continue;
            }
            let screen = worker::capture_worker_screen(&tile, 40);
            let ready_visible = ready_text.as_deref().is_none_or(|t| screen.contains(t));
            let settled = {
                let mut map = guard(quiet_states());
                let next = quiet_step(map.get(&tile), &screen, Instant::now(), ready_visible);
                let changed = map.get(&tile).is_none_or(|p| p.status != next.status);
                let status = next.status;
                map.insert(tile.clone(), next);
                changed.then_some(status)
            };
            if let Some(status) = settled {
                let party = crate::agents::mail::Party::Tile { id: tile.clone() };
                crate::agents::messages::note_status(&party.key(), status == LiveStatus::Busy, status == LiveStatus::Idle);
                if status == LiveStatus::Idle {
                    crate::agents::delivery::kick(&party);
                }
            }
        }
    }
}

pub fn plain_ready_text(binding: &serde_json::Value, profiles: &[Profile]) -> Option<Option<String>> {
    let cli = binding["agent"].as_str()?;
    let profile = profiles.iter().find(|p| p.exe == cli || p.id == cli)?;
    Some(profile.ready_text.clone())
}

fn plain_quiet_targets() -> Vec<(String, Option<String>)> {
    let tiles: std::collections::BTreeSet<String> = crate::agents::messages::with_mailbox(|mb| mb.messages.values()
        .filter(|m| m.status == crate::agents::mail::MsgStatus::Queued)
        .filter_map(|m| match &m.to { crate::agents::mail::Party::Tile { id } => Some(id.clone()), _ => None })
        .collect());
    if tiles.is_empty() {
        return vec![];
    }
    let Some(base) = crate::panorama_dir() else { return vec![] };
    let profiles = load_profiles(&base);
    tiles.into_iter()
        .filter(|t| !hook_status_known(t))
        .filter_map(|t| {
            let binding = crate::read_binding_rec(&t)?;
            plain_ready_text(&binding, &profiles).map(|ready| (t, ready))
        })
        .collect()
}

pub(crate) fn quiet_mail(agent_id: &str) -> bool {
    crate::agents::delivery::has_mail(&crate::agents::mail::Party::Agent { id: agent_id.to_string() })
        || crate::agents::routines::awaiting_turn(agent_id)
        || crate::agents::rollover::paused(agent_id)
}

pub fn quiet_wanted(hook: bool, ui: bool, mail: bool, hook_missing: bool) -> bool {
    if hook { mail && hook_missing } else { ui || mail }
}

fn hook_status_known(tile: &str) -> bool {
    crate::lookup_live(tile)
        .and_then(|s| crate::cached_agent_state(&s))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .is_some_and(|v| v["lastStatus"].is_string())
}

fn status_settled(id: &str) {
    let Some(agent) = agent_snapshot(id) else { return };
    let status = crate::agents::live_view(&agent).status;
    let key = crate::agents::mail::Party::Agent { id: id.to_string() }.key();
    crate::agents::messages::note_status(&key, status == LiveStatus::Busy, status == LiveStatus::Idle);
    crate::agents::rollover::on_settled(id, status);
    if status == LiveStatus::Idle {
        crate::agents::delivery::kick(&crate::agents::mail::Party::Agent { id: id.to_string() });
    }
}

pub fn spawn_background<F>(future: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if let Some(handle) = tokio::runtime::Handle::try_current().ok().or_else(|| RUNTIME.get().cloned()) {
        handle.spawn(future);
    }
}

pub fn spawn_checked<F>(future: F) -> bool
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    match tokio::runtime::Handle::try_current().ok().or_else(|| RUNTIME.get().cloned()) {
        Some(handle) => {
            handle.spawn(future);
            true
        }
        None => false,
    }
}

static RUNTIME: OnceLock<tokio::runtime::Handle> = OnceLock::new();

pub fn remember_runtime(handle: tokio::runtime::Handle) {
    let _ = RUNTIME.set(handle);
}

pub fn status_touched(agent_id: &str) {
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let pending = PENDING.get_or_init(Default::default);
    if !guard(pending).insert(agent_id.to_string()) {
        return;
    }
    let Some(handle) = tokio::runtime::Handle::try_current().ok().or_else(|| RUNTIME.get().cloned()) else {
        guard(pending).remove(agent_id);
        return;
    };
    let id = agent_id.to_string();
    handle.spawn(async move {
        tokio::time::sleep(STATUS_DEBOUNCE).await;
        guard(pending).remove(&id);
        if agent_snapshot(&id).is_some() {
            crate::ui::broadcast_agents_changed(&id);
            status_settled(&id);
        }
    });
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn backoff_follows_the_schedule_then_stops() {
        assert_eq!(backoff(0), Some(Duration::from_secs(5)));
        assert_eq!(backoff(1), Some(Duration::from_secs(30)));
        assert_eq!(backoff(2), Some(Duration::from_secs(120)));
        assert_eq!(backoff(3), None);
    }

    struct FakeBackend {
        agent_id: String,
        calls: Mutex<Vec<&'static str>>,
        live: bool,
        cli_running: bool,
        stop_at: Option<&'static str>,
        wait_result: Result<(), String>,
    }

    impl FakeBackend {
        fn new(agent_id: &str, stop_at: Option<&'static str>) -> Self {
            Self { agent_id: agent_id.into(), calls: Mutex::new(vec![]), live: false, cli_running: false, stop_at, wait_result: Ok(()) }
        }

        fn step(&self, name: &'static str) -> bool {
            self.calls.lock().unwrap().push(name);
            let stop_here = self.stop_at == Some(name);
            if stop_here {
                let cmd = Cmd::Stop { id: self.agent_id.clone(), at: unix_ms(), by: "user".into(), force: true, crashed: false };
                assert!(commit_cmd(&self.agent_id, cmd).is_ok());
            }
            stop_here
        }

        fn calls(&self) -> Vec<&'static str> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl StartBackend for FakeBackend {
        fn live(&self, _: &str) -> bool { self.live }
        async fn cli_running(&self, _: &str) -> bool { self.step("cli_running"); self.cli_running }
        async fn stop_session(&self, _: &str) -> bool { self.step("stop_session"); true }
        async fn list_sessions(&self, _: &str, _: &str) -> Option<HashSet<String>> { self.step("list"); Some(HashSet::new()) }
        async fn ensure_session(&self, _: &crate::Params) -> Result<(), String> { self.step("ensure"); Ok(()) }
        async fn settle(&self) { self.step("settle"); }
        fn send(&self, _: &str, _: &str) -> Result<(), String> { self.step("send"); Ok(()) }
        async fn wait_ready(&self, _: &str, _: &Profile, _: Option<&str>) -> Result<(), String> {
            let killed = self.step("wait") && self.wait_result.is_err();
            if killed { Err("terminal killed".into()) } else { Ok(()) }
        }
        async fn after_fresh_start(&self, _: &Agent, _: &Profile) { self.step("after_fresh"); }
        async fn capture_session(&self, _: &str) { self.step("capture"); }
        fn respawn(&self, _: &str) { self.step("respawn"); }
        fn ready(&self, _: &str) { self.step("ready"); }
    }

    fn running_kimi_agent(tag: &str, session: Option<&str>) -> String {
        crate::orch::tests::f12_test_home();
        let id = format!("a{}{:06}", &tag[..1], unix_ms() % 1_000_000);
        let mut agent = crate::agents::reducer::tests::agent(&id, tag);
        agent.profile = "kimi".into();
        agent.model = None;
        agent.desired = Desired::Running;
        agent.native_session = session.map(|s| crate::agents::model::NativeSession { agent: "kimi".into(), id: s.into(), bound_at: 1, model: None });
        lock().agents.insert(id.clone(), agent);
        id
    }

    fn finish(id: &str) -> Agent {
        let agent = lock().agents.remove(id).unwrap();
        forget(id, &agent.tile_id());
        if let Some(base) = crate::panorama_dir() {
            let _ = store::remove_agent_dir(&base, id);
        }
        agent
    }

    #[tokio::test(start_paused = true)]
    async fn run_start_stopped_while_resuming_keeps_the_native_session() {
        let id = running_kimi_agent("resume", Some("session_keep"));
        let backend = FakeBackend { wait_result: Err("killed".into()), ..FakeBackend::new(&id, Some("wait")) };
        run_start_with(&backend, id.clone()).await;
        let agent = finish(&id);
        let calls = backend.calls();
        assert_eq!(agent.desired, Desired::Stopped);
        assert_eq!(agent.native_session.map(|s| s.id).as_deref(), Some("session_keep"), "{calls:?}");
        assert_eq!(agent.last_exit, None, "{calls:?}");
        assert_eq!(calls.iter().filter(|c| **c == "ensure").count(), 1, "no fresh retry: {calls:?}");
        assert_eq!(calls.last(), Some(&"stop_session"), "{calls:?}");
    }

    #[tokio::test(start_paused = true)]
    async fn run_start_stopped_after_each_await_stops_before_the_next_step() {
        for (stop_at, forbidden) in [("list", "ensure"), ("ensure", "settle"), ("settle", "send"), ("wait", "after_fresh")] {
            let id = running_kimi_agent("fresh", None);
            let backend = FakeBackend::new(&id, Some(stop_at));
            run_start_with(&backend, id.clone()).await;
            let agent = finish(&id);
            let calls = backend.calls();
            assert!(!calls.contains(&forbidden), "stop at {stop_at}: {calls:?}");
            assert_eq!(calls.last(), Some(&"stop_session"), "stop at {stop_at} kills the terminal: {calls:?}");
            assert!(!calls.contains(&"respawn"), "stop at {stop_at}: {calls:?}");
            assert_eq!(agent.desired, Desired::Stopped, "stop at {stop_at}");
            assert_eq!(agent.last_exit, None, "stop at {stop_at}: {calls:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn run_start_replaces_a_plain_shell_under_the_agent_tile() {
        let id = running_kimi_agent("shell", None);
        let backend = FakeBackend { live: true, cli_running: false, ..FakeBackend::new(&id, None) };
        run_start_with(&backend, id.clone()).await;
        finish(&id);
        let calls = backend.calls();
        let stop = calls.iter().position(|c| *c == "stop_session").expect("plain shell is stopped");
        let ensure = calls.iter().position(|c| *c == "ensure").expect("a new session is created");
        assert!(stop < ensure, "{calls:?}");

        let id = running_kimi_agent("keep", None);
        let backend = FakeBackend { live: true, cli_running: true, ..FakeBackend::new(&id, None) };
        run_start_with(&backend, id.clone()).await;
        finish(&id);
        assert_eq!(backend.calls(), vec!["cli_running"], "a running CLI is kept");
    }

    #[test]
    fn finish_restarts_a_start_cancelled_while_the_agent_is_wanted_again() {
        assert_eq!(finish_action(LaunchError::Cancelled, Desired::Running), FinishAction::Respawn);
        assert_eq!(finish_action(LaunchError::Cancelled, Desired::Stopped), FinishAction::StopSession);
        assert_eq!(finish_action(LaunchError::Failed("x".into()), Desired::Stopped), FinishAction::StopSession);
        assert_eq!(finish_action(LaunchError::ResumeFailed("x".into()), Desired::Stopped), FinishAction::StopSession);
        assert_eq!(finish_action(LaunchError::Failed("x".into()), Desired::Running), FinishAction::Crash("x".into()));
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
    fn quiet_detector_runs_for_hook_clis_only_with_mail_and_no_hook_status() {
        assert!(quiet_wanted(false, true, false, true), "kimi/opencode with the UI open");
        assert!(quiet_wanted(false, false, true, true), "kimi/opencode with mail and no UI");
        assert!(!quiet_wanted(false, false, false, true), "kimi/opencode idle and unwatched");
        assert!(quiet_wanted(true, false, true, true), "claude with mail and no status yet");
        assert!(!quiet_wanted(true, true, true, false), "claude with a hook status needs no detector");
        assert!(!quiet_wanted(true, true, false, true), "claude without mail");
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
    fn a_plain_tile_gets_the_quiet_detector_only_when_its_binding_names_a_known_cli() {
        let profiles: Vec<Profile> = vec![serde_json::from_value(serde_json::json!({ "id": "claude", "exe": "claude", "readyText": "? for shortcuts" })).unwrap()];
        assert_eq!(plain_ready_text(&serde_json::json!({ "agent": "claude" }), &profiles), Some(Some("? for shortcuts".into())));
        assert_eq!(plain_ready_text(&serde_json::json!({ "agent": "mystery" }), &profiles), None);
        assert_eq!(plain_ready_text(&serde_json::json!({}), &profiles), None);
    }

    pub(crate) fn set_quiet(tile: &str, status: LiveStatus) {
        guard(quiet_states()).insert(tile.into(), QuietState { screen: String::new(), changed_at: Instant::now(), status });
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
