use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::agents::mail::{MailCmd, Message, MsgStatus, Party, KIND_EVENT};
use crate::agents::messages::{commit_mail, with_mailbox};
use crate::agents::model::Desired;
use crate::agents::{err_resp, lock, ok_resp, store, text_field, unix_ms};
use crate::ProbeSession;

pub const MAX_TARGETS: usize = 4;
pub const OPEN_PER_PARTY: usize = 8;
pub const OPEN_TOTAL: usize = 200;
pub const FILE_WAITS_MAX: usize = 20;
pub const NOTE_MAX_CHARS: usize = 200;
pub const WAKE_MAX_BYTES: usize = 600;
const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;
pub const DEFAULT_TIMEOUT_MS: u64 = 2 * HOUR_MS;
pub const MAX_TIMEOUT_MS: u64 = DAY_MS;
pub const MIN_AHEAD_MS: u64 = MINUTE_MS;
pub const MAX_AHEAD_MS: u64 = 7 * DAY_MS;
pub const WAKES_PER_HOUR: usize = 30;
pub const SETTLE_MS: u64 = 2_000;
pub const TILE_CLOSE_GRACE_MS: u64 = 60_000;
const TICK: Duration = Duration::from_secs(1);
const HOST_CHECK_TICKS: u64 = 5;
const LIST_MAX: usize = 50;
const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
const END_TURN: &str = "End your turn now; the event arrives as a new message.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStat {
    pub size: u64,
    pub mtime: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Run { run: String, #[serde(default)] after: u32 },
    Task { run: String, task: String },
    Session { spec: String, ready: bool, session: String, #[serde(default)] pid: Option<u32> },
    Agent { id: String, name: String },
    Time { at: u64 },
    File { path: String, #[serde(default)] stat: Option<FileStat> },
}

impl Target {
    fn key(&self) -> String {
        match self {
            Target::Run { run, .. } => format!("run:{run}"),
            Target::Task { run, task } => format!("task:{run}/{task}"),
            Target::Session { session, ready, .. } => format!("session:{session}{}", if *ready { ":ready" } else { "" }),
            Target::Agent { id, .. } => format!("agent:{id}"),
            Target::Time { at } => format!("time:{at}"),
            Target::File { path, .. } => format!("file:{path}"),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Target::Run { run, .. } => format!("run {run}"),
            Target::Task { run, task } => format!("task {run}/{task}"),
            Target::Session { spec, ready, .. } => session_label(spec, *ready),
            Target::Agent { name, .. } => format!("agent {name}"),
            Target::Time { at } => format!("timer {}", iso_utc(*at)),
            Target::File { path, .. } => format!("file {path}"),
        }
    }
}

fn session_label(spec: &str, ready: bool) -> String {
    match spec.split_once('@') {
        Some(("build", cwd)) => format!("build in {cwd}"),
        Some(("run", cwd)) if ready => format!("dev run ready in {cwd}"),
        Some(("run", cwd)) => format!("dev run in {cwd}"),
        _ => format!("terminal {}", spec.chars().take(8).collect::<String>()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    Run(String),
    Task(String, String),
    Session { kind: Option<String>, what: String, ready: bool },
    Agent(String),
    Time(u64),
    File(String),
}

pub fn parse_duration(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let split = raw.find(|c: char| !c.is_ascii_digit()).unwrap_or(raw.len());
    let (num, unit) = raw.split_at(split);
    let n: u64 = num.parse().ok()?;
    let mult = match unit.trim() {
        "" | "s" | "sec" => 1_000,
        "m" | "min" => MINUTE_MS,
        "h" => HOUR_MS,
        "d" => DAY_MS,
        _ => return None,
    };
    n.checked_mul(mult)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(month) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

pub fn parse_iso(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let num = |s: &str| s.parse::<u32>().ok();
    let (date, rest) = raw.split_at(raw.find(['T', ' '])?);
    let rest = &rest[1..];
    let mut d = date.split('-');
    let (year, month, day) = (d.next()?.parse::<i64>().ok()?, num(d.next()?)?, num(d.next()?)?);
    if d.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (clock, offset_ms) = if let Some(c) = rest.strip_suffix('Z') {
        (c, 0i64)
    } else if let Some(pos) = rest.rfind(['+', '-']).filter(|p| *p >= 5) {
        let sign = if rest.as_bytes()[pos] == b'-' { -1 } else { 1 };
        let (h, m) = rest[pos + 1..].split_once(':')?;
        (&rest[..pos], sign * (i64::from(num(h)?) * 60 + i64::from(num(m)?)) * 60_000)
    } else {
        (rest, 0)
    };
    let mut t = clock.split(':');
    let (h, m) = (num(t.next()?)?, num(t.next()?)?);
    let s = t.next().map(|s| s.split('.').next().and_then(num)).unwrap_or(Some(0))?;
    if h > 23 || m > 59 || s > 60 || t.next().is_some() {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if civil_from_days(days) != (year, month, day) {
        return None;
    }
    let ms = days * DAY_MS as i64 + (i64::from(h) * 3600 + i64::from(m) * 60 + i64::from(s)) * 1000 - offset_ms;
    u64::try_from(ms).ok()
}

pub fn iso_utc(ms: u64) -> String {
    let days = (ms / DAY_MS) as i64;
    let (y, mo, d) = civil_from_days(days);
    let secs = (ms % DAY_MS) / 1000;
    format!("{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3600, secs / 60 % 60, secs % 60)
}

fn ahead_ok(at: u64, now: u64) -> Result<u64, String> {
    if at > now && at < now + MIN_AHEAD_MS {
        return Err("a timer must be at least 60s ahead".into());
    }
    if at > now + MAX_AHEAD_MS {
        return Err("a timer can be at most 7 days ahead".into());
    }
    Ok(at)
}

pub fn parse_spec(raw: &str, now: u64) -> Result<Spec, String> {
    let raw = raw.trim();
    let (kind, rest) = raw.split_once(':').ok_or_else(|| format!("bad target {raw}: use run:, task:, session:, agent:, time:, in: or file:"))?;
    let rest = rest.trim();
    if rest.is_empty() {
        return Err(format!("bad target {raw}: missing value"));
    }
    match kind {
        "run" => Ok(Spec::Run(rest.into())),
        "task" => {
            let (run, task) = rest.split_once('/').filter(|(r, t)| !r.is_empty() && !t.is_empty())
                .ok_or_else(|| format!("bad target {raw}: use task:<runId>/<taskId>"))?;
            Ok(Spec::Task(run.into(), task.into()))
        }
        "session" => {
            let (body, ready) = match rest.strip_suffix(":ready") {
                Some(body) => (body, true),
                None => (rest, false),
            };
            match body.split_once('@') {
                Some((k @ ("build" | "run"), cwd)) if !cwd.is_empty() => {
                    if ready && k != "run" {
                        return Err(format!("bad target {raw}: :ready applies to session:run@<cwd>"));
                    }
                    Ok(Spec::Session { kind: Some(k.into()), what: cwd.into(), ready })
                }
                _ if ready => Err(format!("bad target {raw}: :ready applies to session:run@<cwd>")),
                _ => Ok(Spec::Session { kind: None, what: body.into(), ready: false }),
            }
        }
        "agent" => Ok(Spec::Agent(rest.into())),
        "time" => Ok(Spec::Time(ahead_ok(parse_iso(rest).ok_or_else(|| format!("bad time {rest}: use 2026-11-25T18:30:00Z"))?, now)?)),
        "in" => {
            let ms = parse_duration(rest).ok_or_else(|| format!("bad duration {rest}: use 90s, 10m, 2h or 1d"))?;
            if ms < MIN_AHEAD_MS {
                return Err("in: must be at least 60s".into());
            }
            Ok(Spec::Time(ahead_ok(now + ms, now)?))
        }
        "file" => Ok(Spec::File(rest.into())),
        _ => Err(format!("bad target {raw}: use run:, task:, session:, agent:, time:, in: or file:")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WaitStatus {
    Open,
    Fired,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fired {
    #[serde(default)]
    pub target: Option<usize>,
    pub at: u64,
    pub summary: String,
    pub wake: bool,
    #[serde(default)]
    pub started: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wait {
    pub id: String,
    pub waiter: Party,
    #[serde(default)]
    pub waiter_name: String,
    pub targets: Vec<Target>,
    #[serde(default)]
    pub note: String,
    pub created_at: u64,
    #[serde(default)]
    pub timeout_at: Option<u64>,
    pub status: WaitStatus,
    #[serde(default)]
    pub fired: Option<Fired>,
    #[serde(default)]
    pub cancelled_by: Option<String>,
}

impl Wait {
    fn same_targets(&self, targets: &[Target]) -> bool {
        let mut a: Vec<String> = self.targets.iter().map(Target::key).collect();
        let mut b: Vec<String> = targets.iter().map(Target::key).collect();
        a.sort();
        b.sort();
        a == b
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitError {
    pub code: &'static str,
    pub message: String,
}

fn werr(code: &'static str, message: impl Into<String>) -> WaitError {
    WaitError { code, message: message.into() }
}

pub enum WaitCmd {
    Open(Wait),
    Fire { id: String, fired: Fired },
    Expire { id: String, fired: Fired },
    Cancel { id: String, by: String, at: u64 },
}

#[derive(Debug, Default)]
pub struct Waits {
    pub waits: BTreeMap<String, Wait>,
}

impl Waits {
    pub fn open(&self) -> impl Iterator<Item = &Wait> {
        self.waits.values().filter(|w| w.status == WaitStatus::Open)
    }

    pub fn open_for<'a>(&'a self, party: &'a Party) -> impl Iterator<Item = &'a Wait> {
        self.open().filter(move |w| w.waiter == *party)
    }

    pub fn duplicate_of(&self, waiter: &Party, targets: &[Target]) -> Option<&Wait> {
        self.open().find(|w| w.waiter == *waiter && w.same_targets(targets))
    }

    pub fn wakes_in_hour(&self, waiter: &Party, now: u64) -> usize {
        self.waits.values()
            .filter(|w| w.waiter == *waiter)
            .filter_map(|w| w.fired.as_ref())
            .filter(|f| now.saturating_sub(f.at) < HOUR_MS)
            .map(|f| usize::from(f.wake) + usize::from(f.started))
            .sum()
    }

    fn open_one(&self, id: &str) -> Result<&Wait, WaitError> {
        let w = self.waits.get(id).ok_or_else(|| werr("not_found", format!("wait {id} not found")))?;
        if w.status != WaitStatus::Open {
            return Err(werr("conflict", format!("wait {id} is not open")));
        }
        Ok(w)
    }

    pub fn apply(&self, cmd: &WaitCmd) -> Result<(&'static str, Value), WaitError> {
        match cmd {
            WaitCmd::Open(w) => {
                if self.waits.contains_key(&w.id) {
                    return Err(werr("conflict", "wait id exists"));
                }
                if w.targets.is_empty() || w.targets.len() > MAX_TARGETS {
                    return Err(werr("bad_request", format!("1 to {MAX_TARGETS} targets")));
                }
                if w.note.chars().count() > NOTE_MAX_CHARS {
                    return Err(werr("bad_request", format!("note is longer than {NOTE_MAX_CHARS} characters")));
                }
                if self.open_for(&w.waiter).count() >= OPEN_PER_PARTY {
                    return Err(werr("limit", format!("{OPEN_PER_PARTY} open waits already; cancel one first")));
                }
                if self.open().count() >= OPEN_TOTAL {
                    return Err(werr("limit", format!("{OPEN_TOTAL} open waits in total")));
                }
                let files = |ts: &[Target]| ts.iter().filter(|t| matches!(t, Target::File { .. })).count();
                let open_files: usize = self.open().map(|o| files(&o.targets)).sum();
                if files(&w.targets) > 0 && open_files + files(&w.targets) > FILE_WAITS_MAX {
                    return Err(werr("limit", format!("{FILE_WAITS_MAX} file waits in total")));
                }
                let mut stored = w.clone();
                stored.status = WaitStatus::Open;
                let data = serde_json::to_value(stored).map_err(|e| werr("io", e.to_string()))?;
                Ok(("wait-open", data))
            }
            WaitCmd::Fire { id, fired } | WaitCmd::Expire { id, fired } => {
                self.open_one(id)?;
                let kind = if matches!(cmd, WaitCmd::Fire { .. }) { "wait-fire" } else { "wait-expire" };
                Ok((kind, json!({ "id": id, "fired": fired })))
            }
            WaitCmd::Cancel { id, by, at } => {
                self.open_one(id)?;
                Ok(("wait-cancel", json!({ "id": id, "by": by, "at": at })))
            }
        }
    }

    pub fn replay(&mut self, kind: &str, data: &Value) -> Result<(), String> {
        match kind {
            "wait-open" => {
                let w: Wait = serde_json::from_value(data.clone()).map_err(|e| e.to_string())?;
                self.waits.insert(w.id.clone(), w);
            }
            "wait-fire" | "wait-expire" => {
                let id = data["id"].as_str().ok_or("missing id")?;
                let fired: Fired = serde_json::from_value(data["fired"].clone()).map_err(|e| e.to_string())?;
                if let Some(w) = self.waits.get_mut(id) {
                    w.status = if kind == "wait-fire" { WaitStatus::Fired } else { WaitStatus::Expired };
                    w.fired = Some(fired);
                }
            }
            "wait-cancel" => {
                let id = data["id"].as_str().ok_or("missing id")?;
                if let Some(w) = self.waits.get_mut(id) {
                    w.status = WaitStatus::Cancelled;
                    w.cancelled_by = data["by"].as_str().map(str::to_string);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn cut(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut end = max.saturating_sub(3);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

pub fn wake_text(wait: &Wait, summary: &str) -> String {
    let note = if wait.note.is_empty() { String::new() } else { format!(" Note: {}", wait.note) };
    let room = WAKE_MAX_BYTES.saturating_sub(note.len());
    format!("{}{note}", cut(summary, room))
}

pub fn add_more(text: &str, n: usize, cli: &str) -> String {
    let (base, had) = match text.rsplit_once("\n+") {
        Some((base, tail)) if tail.contains(" more event") => {
            (base, tail.split_whitespace().next().and_then(|k| k.parse::<usize>().ok()).unwrap_or(0))
        }
        _ => (text, 0),
    };
    let total = had + n;
    let noun = if total == 1 { "event" } else { "events" };
    format!("{base}\n+{total} more {noun}: {cli} agent waits")
}

pub fn session_summary(spec: &str, ready: bool, probe: &ProbeSession, code_hint: Option<u32>) -> Option<String> {
    let label = match spec.split_once('@') {
        Some(("build", _)) => "build",
        Some(("run", _)) => "dev run",
        _ => "terminal",
    };
    let logs = spec.split_once('@').map(|(kind, cwd)| format!(" Logs: {}", crate::run_logs_hint(kind, cwd))).unwrap_or_default();
    match probe {
        ProbeSession::Running { ready: Some(reason) } if ready => Some(format!("dev run ready: {reason}.")),
        ProbeSession::Running { .. } | ProbeSession::Unknown => None,
        ProbeSession::Exited { code, stopped_by_user, last_line, cmd } => {
            let code = code.or(code_hint).map_or("with exit code unknown".to_string(), |c| format!("{c}"));
            let what = if cmd.is_empty() { label.to_string() } else { format!("{label} `{}`", cut(cmd, 80)) };
            let prefix = if ready { "dev run exited before ready: " } else { "" };
            let by_user = if *stopped_by_user { ", stopped by user" } else { "" };
            let line = if last_line.is_empty() { String::new() } else { format!(" Last line: {}", cut(last_line, 200)) };
            Some(format!("{prefix}{what} exited {code}{by_user}.{line}{logs}"))
        }
        ProbeSession::Gone => Some(format!("{label} {} is gone (exit code unknown).{logs}", session_label(spec, false).split(' ').skip(1).collect::<Vec<_>>().join(" "))),
    }
}

struct Runtime {
    waits: Waits,
    log_seq: u32,
    agent_busy: HashSet<String>,
    pending_more: HashMap<String, usize>,
    exit_codes: HashMap<String, Option<u32>>,
    booted_at: u64,
    closed_checked: bool,
}

fn runtime() -> &'static Mutex<Runtime> {
    static RT: OnceLock<Mutex<Runtime>> = OnceLock::new();
    RT.get_or_init(|| Mutex::new(Runtime {
        waits: Waits::default(), log_seq: 0, agent_busy: HashSet::new(), pending_more: HashMap::new(),
        exit_codes: HashMap::new(), booted_at: unix_ms(), closed_checked: false,
    }))
}

fn rt() -> std::sync::MutexGuard<'static, Runtime> {
    runtime().lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn with_waits<T>(f: impl FnOnce(&Waits) -> T) -> T {
    f(&rt().waits)
}

fn waiter_key(cmd: &WaitCmd, waits: &Waits) -> Option<String> {
    let id = match cmd {
        WaitCmd::Open(w) => return Some(w.waiter.key()),
        WaitCmd::Fire { id, .. } | WaitCmd::Expire { id, .. } | WaitCmd::Cancel { id, .. } => id,
    };
    waits.waits.get(id).map(|w| w.waiter.key())
}

fn commit(cmd: WaitCmd) -> Result<Value, WaitError> {
    let io = |m: String| werr("io", m);
    let (data, key) = {
        let mut st = rt();
        let (kind, data) = st.waits.apply(&cmd)?;
        let key = waiter_key(&cmd, &st.waits);
        let base = crate::panorama_dir().ok_or_else(|| io("no panorama dir".into()))?;
        let path = store::waits_path(&base);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(e.to_string()))?;
        }
        let seq = st.log_seq + 1;
        let record = store::EventRecord { seq, at: unix_ms(), kind: kind.into(), data: data.clone() };
        store::append_event(&path, &record).map_err(io)?;
        st.log_seq = seq;
        if let Err(e) = st.waits.replay(kind, &data) {
            eprintln!("[waits] replay after commit {kind}: {e}");
        }
        (data, key)
    };
    crate::ui::broadcast_agents_waits(key.as_deref().unwrap_or(""));
    Ok(data)
}

pub fn init() {
    let Some(base) = crate::panorama_dir() else { return };
    if let Ok(bytes) = std::fs::read(store::waits_path(&base)) {
        let mut st = rt();
        for line in bytes.split(|b| *b == b'\n') {
            let Ok(text) = std::str::from_utf8(line) else { continue };
            let Ok(record) = serde_json::from_str::<store::EventRecord>(text.trim()) else { continue };
            st.log_seq = st.log_seq.max(record.seq);
            if let Err(e) = st.waits.replay(&record.kind, &record.data) {
                eprintln!("[waits] skip event {}: {e}", record.seq);
            }
        }
        st.booted_at = unix_ms();
    }
    repair_unsent_wakes();
    crate::agents::runtime::spawn_background(tick_loop());
    crate::agents::runtime::spawn_background(file_poll_loop());
}

pub(crate) fn unsent_wakes(waits: &Waits, has_message: impl Fn(&str) -> bool) -> Vec<(Wait, String)> {
    waits.waits.values()
        .filter(|w| matches!(w.status, WaitStatus::Fired | WaitStatus::Expired))
        .filter_map(|w| w.fired.as_ref().filter(|f| f.wake).map(|f| (w.clone(), f.summary.clone())))
        .filter(|(w, _)| !has_message(&wake_id(&w.id)))
        .collect()
}

fn repair_unsent_wakes() {
    let missing = with_waits(|w| unsent_wakes(w, |id| with_mailbox(|mb| mb.messages.contains_key(id))));
    for (wait, summary) in missing {
        send_wake(&wait, &summary);
    }
}

pub fn wake_id(wait_id: &str) -> String {
    format!("e{wait_id}")
}

fn gen_wait_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::getrandom(&mut bytes).expect("os rng");
    let mut id = String::from("w");
    id.extend(bytes.iter().map(|b| ID_ALPHABET[*b as usize % ID_ALPHABET.len()] as char));
    id
}

#[derive(Debug, PartialEq, Eq)]
pub struct WakePlan {
    pub wake: bool,
    pub start: bool,
}

pub fn wake_plan(used: usize, stopped_agent: Option<bool>) -> WakePlan {
    let wake = used < WAKES_PER_HOUR;
    let start = wake && stopped_agent == Some(false) && used + 2 <= WAKES_PER_HOUR;
    WakePlan { wake, start }
}

fn stopped_agent(waiter: &Party) -> Option<bool> {
    let id = waiter.agent_id()?;
    let agents = lock();
    let agent = agents.agents.get(id)?;
    (agent.desired == Desired::Stopped).then_some(agent.crashed)
}

pub(crate) fn fire(id: &str, target: Option<usize>, summary: String, expired: bool) -> bool {
    let now = unix_ms();
    let Some((wait, used)) = ({
        let st = rt();
        st.waits.waits.get(id).filter(|w| w.status == WaitStatus::Open).map(|w| (w.clone(), st.waits.wakes_in_hour(&w.waiter, now)))
    }) else {
        return false;
    };
    let plan = wake_plan(used, stopped_agent(&wait.waiter));
    let fired = Fired { target, at: now, summary: summary.clone(), wake: plan.wake, started: plan.start };
    let cmd = if expired { WaitCmd::Expire { id: id.into(), fired } } else { WaitCmd::Fire { id: id.into(), fired } };
    if let Err(e) = commit(cmd) {
        if e.code != "conflict" {
            eprintln!("[waits] fire {id}: {}", e.message);
        }
        return false;
    }
    rt().agent_busy.remove(id);
    if plan.wake {
        send_wake(&wait, &summary);
    } else {
        merge_more(&wait.waiter);
    }
    if plan.start {
        if let Some(agent) = wait.waiter.agent_id() {
            if let Err(e) = crate::agents::start_agent(agent, Some(format!("wait {}", wait.id))) {
                eprintln!("[waits] auto-start for {}: {}", wait.id, e.message);
            }
        }
    }
    true
}

fn take_more(waiter: &Party) -> usize {
    rt().pending_more.remove(&waiter.key()).unwrap_or(0)
}

fn send_wake(wait: &Wait, summary: &str) {
    let mut text = wake_text(wait, summary);
    let more = take_more(&wait.waiter);
    if more > 0 {
        text = add_more(&text, more, &crate::panorama_cli_path());
    }
    let now = unix_ms();
    let message = Message {
        id: wake_id(&wait.id), from: Party::Brain, to: wait.waiter.clone(), from_name: "panorama".into(), text,
        expects_reply: false, reply_to: None, thread: wait.id.clone(), hops: 0, at: now, seq: 0, status: MsgStatus::Queued,
        delivered_at: None, replied_by: None, reason: None, read: false, notice: false, kind: Some(KIND_EVENT.into()), key: None,
    };
    let to = message.to.clone();
    let queued = message.clone();
    match commit_mail(MailCmd::Send { message, recipient_exists: true }) {
        Ok(_) if to == Party::User => {
            if let Err(e) = commit_mail(MailCmd::Deliver { ids: vec![wake_id(&wait.id)], at: now }) {
                eprintln!("[waits] panel delivery of {}: {}", wait.id, e.message);
            }
        }
        Ok(_) => crate::agents::delivery::kick_for(&queued),
        Err(e) if e.code == "conflict" => {}
        Err(e) => eprintln!("[waits] wake for {}: {}", wait.id, e.message),
    }
}

fn merge_more(waiter: &Party) {
    let queued = with_mailbox(|mb| mb.queue_for(waiter).into_iter()
        .filter(|m| m.kind.as_deref() == Some(KIND_EVENT))
        .last()
        .map(|m| (m.id.clone(), m.text.clone())));
    if let Some((id, text)) = queued {
        let text = add_more(&text, 1, &crate::panorama_cli_path());
        if commit_mail(MailCmd::Update { id, text, key: None, at: unix_ms() }).is_ok() {
            return;
        }
    }
    *rt().pending_more.entry(waiter.key()).or_default() += 1;
}

pub(crate) fn cancel_for(party: &Party, by: &str) -> usize {
    let ids: Vec<String> = with_waits(|w| w.open_for(party).map(|w| w.id.clone()).collect());
    ids.iter().filter(|id| commit(WaitCmd::Cancel { id: (*id).clone(), by: by.into(), at: unix_ms() }).is_ok()).count()
}

fn agent_state(id: &str) -> Option<(String, Desired, bool)> {
    lock().agents.get(id).map(|a| (a.name.clone(), a.desired, a.crashed))
}

pub(crate) fn check_target(target: &Target, now: u64, ask_host: bool, exit_codes: &HashMap<String, Option<u32>>) -> Option<String> {
    match target {
        Target::Time { at } => (*at <= now).then(|| "timer.".to_string()),
        Target::Session { spec, ready, session, pid } => {
            let known = exit_codes.get(session).copied();
            let probe = match crate::wait_session_probe(session, *pid, ask_host) {
                ProbeSession::Unknown | ProbeSession::Gone if known.is_some() => {
                    ProbeSession::Exited { code: known.flatten(), stopped_by_user: false, last_line: String::new(), cmd: String::new() }
                }
                probe => probe,
            };
            session_summary(spec, *ready, &probe, known.flatten())
        }
        Target::Agent { id, name } => match agent_state(id) {
            None => Some(format!("agent {name} was deleted.")),
            Some((_, Desired::Stopped, true)) => Some(format!("agent {name} crashed.")),
            Some((_, Desired::Stopped, false)) => Some(format!("agent {name} stopped.")),
            Some(_) => None,
        },
        Target::Run { run, after } => crate::orch::wait_run_event(run, *after),
        Target::Task { run, task } => crate::orch::wait_task_event(run, task),
        Target::File { path, stat } => file_cache_get(path).and_then(|now| file_summary(path, *stat, now)),
    }
}

pub fn local_disk_path(path: &str) -> bool {
    let b = path.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/');
    let unix = b.first() == Some(&b'/') && b.get(1) != Some(&b'/') && b.get(1) != Some(&b'\\');
    drive || unix
}

const FILE_POLL: Duration = Duration::from_secs(5);
const FILE_STAT_TIMEOUT: Duration = Duration::from_secs(2);

fn file_cache() -> &'static Mutex<HashMap<String, Option<FileStat>>> {
    static MAP: OnceLock<Mutex<HashMap<String, Option<FileStat>>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn file_cache_get(path: &str) -> Option<Option<FileStat>> {
    file_cache().lock().unwrap_or_else(|e| e.into_inner()).get(path).copied()
}

fn watched_files() -> Vec<String> {
    with_waits(|w| w.open().flat_map(|w| w.targets.iter()).filter_map(|t| match t {
        Target::File { path, .. } => Some(path.clone()),
        _ => None,
    }).collect())
}

fn keep_cached(paths: &[String]) {
    file_cache().lock().unwrap_or_else(|e| e.into_inner()).retain(|p, _| paths.contains(p));
}

#[cfg(test)]
pub(crate) fn poll_files_once() {
    let paths = watched_files();
    keep_cached(&paths);
    for path in paths {
        let stat = file_stat(&path);
        file_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(path, stat);
    }
}

async fn file_poll_loop() {
    let mut interval = tokio::time::interval(FILE_POLL);
    loop {
        interval.tick().await;
        let paths = watched_files();
        keep_cached(&paths);
        for path in paths {
            let p = path.clone();
            if let Ok(Ok(stat)) = tokio::time::timeout(FILE_STAT_TIMEOUT, tokio::task::spawn_blocking(move || file_stat(&p))).await {
                file_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(path, stat);
            }
        }
    }
}

pub fn file_stat(path: &str) -> Option<FileStat> {
    let meta = std::fs::metadata(path).ok()?;
    let mtime = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
    Some(FileStat { size: meta.len(), mtime })
}

pub fn file_summary(path: &str, before: Option<FileStat>, now: Option<FileStat>) -> Option<String> {
    match (before, now) {
        (None, Some(s)) => Some(format!("file appeared: {path} ({} bytes).", s.size)),
        (Some(_), None) => Some(format!("file removed: {path}.")),
        (Some(a), Some(b)) if a != b => Some(format!("file changed: {path} ({} bytes).", b.size)),
        _ => None,
    }
}

fn timeout_summary(wait: &Wait) -> String {
    let span = wait.timeout_at.unwrap_or(wait.created_at).saturating_sub(wait.created_at);
    let span = if span % HOUR_MS == 0 { format!("{}h", span / HOUR_MS) } else { format!("{}m", span / MINUTE_MS) };
    let on: Vec<String> = wait.targets.iter().map(Target::describe).collect();
    format!("timed out after {span}; still waiting on {}.", on.join(", "))
}

fn prune_exit_codes() {
    let watched: HashSet<String> = with_waits(|w| w.open().flat_map(|w| w.targets.iter()).filter_map(|t| match t {
        Target::Session { session, .. } => Some(session.clone()),
        _ => None,
    }).collect());
    rt().exit_codes.retain(|k, _| watched.contains(k));
}

pub(crate) fn tick_once(now: u64, ask_host: bool) {
    let (open, codes) = {
        let st = rt();
        (st.waits.open().cloned().collect::<Vec<_>>(), st.exit_codes.clone())
    };
    for wait in open {
        if wait.timeout_at.is_some_and(|t| now >= t) {
            fire(&wait.id, None, timeout_summary(&wait), true);
            continue;
        }
        for (i, target) in wait.targets.iter().enumerate() {
            if let Some(summary) = check_target(target, now, ask_host, &codes) {
                fire(&wait.id, Some(i), summary, false);
                break;
            }
        }
    }
    prune_exit_codes();
}

async fn tick_loop() {
    let mut interval = tokio::time::interval(TICK);
    let mut n: u64 = 0;
    loop {
        interval.tick().await;
        n += 1;
        let ask_host = n % HOST_CHECK_TICKS == 0;
        let _ = tokio::task::spawn_blocking(move || {
            tick_once(unix_ms(), ask_host);
            close_stale_tiles(unix_ms());
        }).await;
    }
}

fn close_stale_tiles(now: u64) {
    {
        let mut st = rt();
        if st.closed_checked || now.saturating_sub(st.booted_at) < TILE_CLOSE_GRACE_MS {
            return;
        }
        st.closed_checked = true;
    }
    let tiles: HashSet<String> = with_mailbox(|mb| mb.messages.values()
        .filter(|m| m.status == MsgStatus::Queued)
        .filter_map(|m| match &m.to { Party::Tile { id } => Some(id.clone()), _ => None })
        .collect());
    for tile in tiles {
        if crate::lookup_live(&tile).is_none() && crate::host_session_alive(&tile) == Some(false) {
            crate::agents::on_tile_closed(&tile);
        }
    }
}

pub(crate) fn on_session_exit(session: &str, code: Option<u32>) {
    let hits: Vec<(String, usize, Target)> = with_waits(|w| w.open()
        .flat_map(|wait| wait.targets.iter().enumerate().map(move |(i, t)| (wait.id.clone(), i, t.clone())))
        .filter(|(_, _, t)| matches!(t, Target::Session { session: s, .. } if s == session))
        .collect());
    if hits.is_empty() {
        return;
    }
    rt().exit_codes.insert(session.to_string(), code);
    let codes = rt().exit_codes.clone();
    for (id, i, target) in hits {
        if let Some(summary) = check_target(&target, unix_ms(), false, &codes) {
            fire(&id, Some(i), summary, false);
        }
    }
}

pub(crate) fn on_orch(run_id: &str) {
    let hits: Vec<(String, usize, Target)> = with_waits(|w| w.open()
        .flat_map(|wait| wait.targets.iter().enumerate().map(move |(i, t)| (wait.id.clone(), i, t.clone())))
        .filter(|(_, _, t)| matches!(t, Target::Run { run, .. } | Target::Task { run, .. } if run == run_id))
        .collect());
    for (id, i, target) in hits {
        if let Some(summary) = check_target(&target, unix_ms(), false, &HashMap::new()) {
            fire(&id, Some(i), summary, false);
        }
    }
}

pub(crate) fn open_waits_json(party: &Party) -> Value {
    with_waits(|w| w.open_for(party).map(|w| json!({
        "id": w.id, "on": w.targets.iter().map(Target::describe).collect::<Vec<_>>(), "note": w.note, "timeoutAt": w.timeout_at,
    })).collect::<Vec<_>>().into())
}

pub(crate) fn orch_notice(agent_id: &str, run: &str, text: &str) -> Result<(), String> {
    let id = format!("o{}", &gen_wait_id()[1..]);
    let message = Message {
        id: id.clone(), from: Party::Brain, to: Party::Agent { id: agent_id.into() }, from_name: "panorama".into(), text: text.into(),
        expects_reply: false, reply_to: None, thread: id, hops: 0, at: unix_ms(), seq: 0, status: MsgStatus::Queued,
        delivered_at: None, replied_by: None, reason: None, read: false, notice: false,
        kind: Some(crate::agents::mail::KIND_ORCH.into()), key: Some(format!("{}0", orch_base(agent_id, run))),
    };
    let queued = message.clone();
    commit_mail(MailCmd::Send { message, recipient_exists: true }).map_err(|e| e.message)?;
    crate::agents::delivery::kick_for(&queued);
    Ok(())
}

fn orch_base(agent_id: &str, run: &str) -> String {
    format!("orch:{agent_id}:{run}#")
}

pub(crate) fn orch_fingerprint(agent_id: &str, run: &str) -> u32 {
    let base = orch_base(agent_id, run);
    with_mailbox(|mb| mb.messages.values()
        .filter_map(|m| m.key.as_deref()?.strip_prefix(base.as_str())?.parse::<u32>().ok())
        .max()
        .unwrap_or(0))
}

#[derive(Debug, PartialEq, Eq)]
pub enum OrchWake {
    Skipped,
    Coalesced(String),
    Deferred(u64),
    Sent(String),
}

pub const ORCH_MIN_GAP_MS: u64 = 60_000;

fn orch_recheck_later(agent_id: &str, run: &str, delay_ms: u64) {
    static PENDING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let pending = PENDING.get_or_init(Default::default);
    let key = orch_base(agent_id, run);
    if !pending.lock().unwrap_or_else(|e| e.into_inner()).insert(key.clone()) {
        return;
    }
    let (run, tile) = (run.to_string(), crate::agents::model::tile_id(agent_id));
    crate::agents::runtime::spawn_background(async move {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&key);
        crate::orch::recheck_coordinator(&run, &tile);
    });
}

pub(crate) fn orch_wake(agent_id: &str, run: &str, top: u32, text: &str) -> OrchWake {
    if top <= orch_fingerprint(agent_id, run) {
        return OrchWake::Skipped;
    }
    let base = orch_base(agent_id, run);
    let key = format!("{base}{top}");
    let party = Party::Agent { id: agent_id.to_string() };
    let queued = with_mailbox(|mb| mb.queue_for(&party).into_iter()
        .find(|m| m.key.as_deref().is_some_and(|k| k.starts_with(base.as_str())))
        .map(|m| m.id.clone()));
    if let Some(id) = queued {
        match commit_mail(MailCmd::Update { id: id.clone(), text: text.into(), key: Some(key.clone()), at: unix_ms() }) {
            Ok(_) => return OrchWake::Coalesced(id),
            Err(e) => eprintln!("[waits] orch update {id}: {}", e.message),
        }
    }
    let now = unix_ms();
    let last = with_mailbox(|mb| mb.messages.values()
        .filter(|m| m.to == party && m.key.as_deref().is_some_and(|k| k.starts_with(base.as_str())))
        .filter_map(|m| m.delivered_at)
        .max());
    if let Some(gap) = last.map(|at| now.saturating_sub(at)).filter(|g| *g < ORCH_MIN_GAP_MS) {
        orch_recheck_later(agent_id, run, ORCH_MIN_GAP_MS - gap);
        return OrchWake::Deferred(ORCH_MIN_GAP_MS - gap);
    }
    let id = format!("o{}", &gen_wait_id()[1..]);
    let message = Message {
        id: id.clone(), from: Party::Brain, to: party.clone(), from_name: "panorama".into(), text: text.into(),
        expects_reply: false, reply_to: None, thread: id.clone(), hops: 0, at: unix_ms(), seq: 0, status: MsgStatus::Queued,
        delivered_at: None, replied_by: None, reason: None, read: false, notice: false,
        kind: Some(crate::agents::mail::KIND_ORCH.into()), key: Some(key),
    };
    let queued = message.clone();
    if let Err(e) = commit_mail(MailCmd::Send { message, recipient_exists: true }) {
        eprintln!("[waits] orch wake for {agent_id}: {}", e.message);
        return OrchWake::Skipped;
    }
    if stopped_agent(&party) == Some(false) {
        if let Err(e) = crate::agents::start_agent(agent_id, Some(format!("orch {run}"))) {
            eprintln!("[waits] auto-start coordinator {agent_id}: {}", e.message);
        }
    }
    crate::agents::delivery::kick_for(&queued);
    OrchWake::Sent(id)
}

pub(crate) fn on_party_status(key: &str, busy: bool, idle: bool) {
    let watching: Vec<(String, usize, String)> = with_waits(|w| w.open()
        .flat_map(|wait| wait.targets.iter().enumerate().filter_map(move |(i, t)| match t {
            Target::Agent { id, name } if Party::Agent { id: id.clone() }.key() == key => Some((wait.id.clone(), i, name.clone())),
            _ => None,
        }))
        .collect());
    for (wait_id, i, name) in watching {
        if busy {
            rt().agent_busy.insert(wait_id);
        } else if idle && rt().agent_busy.contains(&wait_id) {
            fire(&wait_id, Some(i), format!("agent {name} finished its turn and is idle."), false);
        }
    }
}

fn agent_busy_now(id: &str) -> bool {
    let Some(agent) = lock().agents.get(id).cloned() else { return false };
    crate::agents::live_view(&agent).status == crate::agents::model::LiveStatus::Busy
}

fn resolve(spec: Spec, waiter: &Party) -> Result<Target, WaitError> {
    match spec {
        Spec::Time(at) => Ok(Target::Time { at }),
        Spec::Agent(name) => {
            let agent = lock().resolve(&name).cloned().ok_or_else(|| werr("not_found", format!("no agent named {name}")))?;
            if waiter.agent_id() == Some(agent.id.as_str()) {
                return Err(werr("bad_request", "an agent cannot wait on itself"));
            }
            Ok(Target::Agent { id: agent.id, name: agent.name })
        }
        Spec::Session { kind, what, ready } => {
            let (session, pid) = crate::wait_session_resolve(kind.as_deref(), &what).ok_or_else(|| {
                let label = kind.as_deref().map_or("terminal".to_string(), |k| format!("{} session in", if k == "build" { "build" } else { "dev run" }));
                werr("not_found", format!("no {label} {what}"))
            })?;
            let spec = match &kind { Some(k) => format!("{k}@{what}"), None => what };
            Ok(Target::Session { spec, ready, session, pid: Some(pid) })
        }
        Spec::Run(run) => {
            let after = crate::orch::wait_run_baseline(&run).ok_or_else(|| werr("not_found", format!("run {run} not found")))?;
            Ok(Target::Run { run, after })
        }
        Spec::Task(run, task) => {
            if !crate::orch::wait_task_known(&run, &task) {
                return Err(werr("not_found", format!("task {run}/{task} not found")));
            }
            Ok(Target::Task { run, task })
        }
        Spec::File(path) => {
            if !local_disk_path(&path) || !std::path::Path::new(&path).is_absolute() {
                return Err(werr("bad_request", "file: needs an absolute path on a local disk (no network or device paths)"));
            }
            let stat = file_stat(&path);
            Ok(Target::File { path, stat })
        }
    }
}

fn wait_json(w: &Wait) -> Value {
    let mut v = serde_json::to_value(w).unwrap_or_default();
    v["on"] = w.targets.iter().map(Target::describe).collect::<Vec<_>>().into();
    v
}

pub fn handle_wait(v: &Value) -> String {
    if v.get("worker").and_then(Value::as_bool) == Some(true) {
        return err_resp("forbidden", "workers do not wait");
    }
    let (waiter, waiter_name) = crate::agents::messages::sender_from(v);
    let raw: Vec<String> = match &v["on"] {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => vec![],
    };
    if raw.is_empty() || raw.len() > MAX_TARGETS {
        return err_resp("bad_request", &format!("give 1 to {MAX_TARGETS} --on targets"));
    }
    let note = text_field(v, "note").unwrap_or_default().trim().to_string();
    if note.chars().count() > NOTE_MAX_CHARS {
        return err_resp("bad_request", &format!("note is longer than {NOTE_MAX_CHARS} characters"));
    }
    let timeout = v.get("timeoutMs").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT_MS);
    if timeout == 0 || timeout > MAX_TIMEOUT_MS {
        return err_resp("bad_request", "timeout must be between 1s and 24h");
    }
    let now = unix_ms();
    let mut targets = Vec::new();
    for r in &raw {
        let target = parse_spec(r, now).map_err(|e| werr("bad_request", e)).and_then(|s| resolve(s, &waiter));
        match target {
            Ok(t) => targets.push(t),
            Err(e) => return err_resp(e.code, &e.message),
        }
    }
    let codes = rt().exit_codes.clone();
    for target in &targets {
        if let Some(summary) = check_target(target, now, false, &codes) {
            return ok_resp(json!({ "fired": true, "event": { "on": target.describe(), "summary": summary }, "text": summary }));
        }
    }
    let end = if waiter == Party::User { String::new() } else { format!(" {END_TURN}") };
    if let Some(existing) = with_waits(|w| w.duplicate_of(&waiter, &targets).cloned()) {
        let text = format!("Already waiting: {} on {}.{end}", existing.id, existing.targets.iter().map(Target::describe).collect::<Vec<_>>().join(", "));
        return ok_resp(json!({ "wait": wait_json(&existing), "existing": true, "text": text }));
    }
    let timers_only = targets.iter().all(|t| matches!(t, Target::Time { .. }));
    let wait = Wait {
        id: gen_wait_id(), waiter: waiter.clone(), waiter_name, targets, note, created_at: now,
        timeout_at: (!timers_only).then_some(now + timeout), status: WaitStatus::Open, fired: None, cancelled_by: None,
    };
    if let Err(e) = commit(WaitCmd::Open(wait.clone())) {
        return err_resp(e.code, &e.message);
    }
    for target in &wait.targets {
        if let Target::Agent { id, .. } = target {
            if agent_busy_now(id) {
                rt().agent_busy.insert(wait.id.clone());
            }
        }
    }
    let on: Vec<String> = wait.targets.iter().map(Target::describe).collect();
    let text = format!("Waiting {} on {}.{end}", wait.id, on.join(", "));
    ok_resp(json!({ "wait": wait_json(&wait), "text": text }))
}

pub fn handle_waits(v: &Value) -> String {
    let party = match text_field(v, "agent") {
        Some(name) => match lock().resolve(&name) {
            Some(a) => Some(Party::Agent { id: a.id.clone() }),
            None => return err_resp("not_found", "agent not found"),
        },
        None if v.get("mine").and_then(Value::as_bool) == Some(true) => Some(crate::agents::messages::sender_from(v).0),
        None => None,
    };
    let status = text_field(v, "status");
    let list: Vec<Value> = with_waits(|w| {
        let mut all: Vec<&Wait> = w.waits.values()
            .filter(|w| party.as_ref().is_none_or(|p| w.waiter == *p))
            .filter(|w| status.as_ref().is_none_or(|s| serde_json::to_value(w.status).ok().and_then(|x| x.as_str().map(str::to_string)).as_deref() == Some(s)))
            .collect();
        all.sort_by_key(|w| (w.status != WaitStatus::Open, std::cmp::Reverse(w.fired.as_ref().map_or(w.created_at, |f| f.at))));
        all.into_iter().take(LIST_MAX).map(wait_json).collect()
    });
    ok_resp(json!({ "waits": list }))
}

pub fn handle_wait_cancel(v: &Value) -> String {
    let (sender, _) = crate::agents::messages::sender_from(v);
    let by = if sender == Party::User { "user".to_string() } else { sender.key() };
    if v.get("all").and_then(Value::as_bool) == Some(true) {
        return ok_resp(json!({ "cancelled": cancel_for(&sender, &by) }));
    }
    let id = text_field(v, "id").unwrap_or_default();
    let Some(wait) = with_waits(|w| w.waits.get(&id).cloned()) else {
        return err_resp("not_found", &format!("wait {id} not found"));
    };
    if sender != Party::User && wait.waiter != sender {
        return err_resp("forbidden", "you can cancel only your own waits");
    }
    match commit(WaitCmd::Cancel { id, by, at: unix_ms() }) {
        Ok(_) => ok_resp(json!({ "cancelled": 1 })),
        Err(e) => err_resp(e.code, &e.message),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::agents::mail::tests::agent;

    pub(crate) const NOW: u64 = 1_800_000_000_000;

    pub(crate) fn wait(id: &str, waiter: Party, targets: Vec<Target>) -> Wait {
        Wait {
            id: id.into(), waiter, waiter_name: "x".into(), targets, note: String::new(), created_at: NOW,
            timeout_at: Some(NOW + DEFAULT_TIMEOUT_MS), status: WaitStatus::Open, fired: None, cancelled_by: None,
        }
    }

    fn timer(at: u64) -> Target {
        Target::Time { at }
    }

    fn run(w: &mut Waits, cmd: WaitCmd) -> Result<(), WaitError> {
        let (kind, data) = w.apply(&cmd)?;
        w.replay(kind, &data).unwrap();
        Ok(())
    }

    fn fired(at: u64, wake: bool) -> Fired {
        Fired { target: Some(0), at, summary: "timer.".into(), wake, started: false }
    }

    #[test]
    fn every_target_form_parses() {
        let p = |s: &str| parse_spec(s, NOW);
        assert_eq!(p("run:r6un"), Ok(Spec::Run("r6un".into())));
        assert_eq!(p("task:r6un/t4"), Ok(Spec::Task("r6un".into(), "t4".into())));
        assert_eq!(p("session:tile1234"), Ok(Spec::Session { kind: None, what: "tile1234".into(), ready: false }));
        assert_eq!(p("session:build@D:/x/y"), Ok(Spec::Session { kind: Some("build".into()), what: "D:/x/y".into(), ready: false }));
        assert_eq!(p("session:run@D:\\x:ready"), Ok(Spec::Session { kind: Some("run".into()), what: "D:\\x".into(), ready: true }));
        assert_eq!(p("agent:rev"), Ok(Spec::Agent("rev".into())));
        assert_eq!(p("in:2m"), Ok(Spec::Time(NOW + 2 * MINUTE_MS)));
        assert_eq!(p("in:90"), Ok(Spec::Time(NOW + 90_000)));
        assert_eq!(p("file:D:/x/out.txt"), Ok(Spec::File("D:/x/out.txt".into())));
        let at = NOW + HOUR_MS;
        assert_eq!(p(&format!("time:{}", iso_utc(at))), Ok(Spec::Time(at)));
        assert_eq!(parse_iso("2026-11-25T18:30:00+02:00"), parse_iso("2026-11-25T16:30:00Z"));
        assert_eq!(parse_iso("2026-11-25T16:30Z"), parse_iso("2026-11-25 16:30:00"));
    }

    #[test]
    fn bad_targets_are_refused() {
        let p = |s: &str| parse_spec(s, NOW);
        assert!(p("in:59s").unwrap_err().contains("60s"));
        assert!(p("in:0s").is_err(), "in: under 60s is refused even when it would be due now");
        assert!(p("in:8d").unwrap_err().contains("7 days"));
        assert!(p(&format!("time:{}", iso_utc(NOW + 30_000))).unwrap_err().contains("60s"));
        assert!(p(&format!("time:{}", iso_utc(NOW + 8 * DAY_MS))).is_err());
        assert_eq!(p(&format!("time:{}", iso_utc(NOW - 5_000))), Ok(Spec::Time(NOW - 5_000)), "a past time fires inline");
        for bad in ["run:", "task:r1", "session:build@D:/x:ready", "foo:bar", "nothing", "time:yesterday", "in:soon", "time:2026-02-30T10:00:00Z"] {
            assert!(p(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn caps_on_targets_note_and_open_waits() {
        let mut w = Waits::default();
        let me = agent("a");
        let five: Vec<Target> = (0..5).map(|i| timer(NOW + i)).collect();
        assert_eq!(w.apply(&WaitCmd::Open(wait("w0", me.clone(), five))).unwrap_err().code, "bad_request");
        let noted = Wait { note: "n".repeat(NOTE_MAX_CHARS + 1), ..wait("w0", me.clone(), vec![timer(NOW)]) };
        assert_eq!(w.apply(&WaitCmd::Open(noted)).unwrap_err().code, "bad_request");
        for i in 0..OPEN_PER_PARTY {
            run(&mut w, WaitCmd::Open(wait(&format!("w{i}"), me.clone(), vec![timer(NOW + i as u64)]))).unwrap();
        }
        assert_eq!(w.apply(&WaitCmd::Open(wait("w9", me.clone(), vec![timer(NOW + 99)]))).unwrap_err().code, "limit");
        run(&mut w, WaitCmd::Open(wait("wb", agent("b"), vec![timer(NOW)]))).unwrap();
        assert_eq!(w.duplicate_of(&me, &[timer(NOW + 3)]).map(|x| x.id.as_str()), Some("w3"));
        assert!(w.duplicate_of(&agent("b"), &[timer(NOW + 3)]).is_none());
        let files: Vec<Wait> = (0..FILE_WAITS_MAX / 4 + 1).map(|i| wait(&format!("f{i}"), agent(&format!("f{i}")),
            (0..4).map(|j| Target::File { path: format!("p{i}{j}"), stat: None }).collect())).collect();
        let mut ok = 0;
        for f in files {
            if run(&mut w, WaitCmd::Open(f)).is_ok() { ok += 1; }
        }
        assert_eq!(ok, FILE_WAITS_MAX / 4, "at most {FILE_WAITS_MAX} file targets in total");
    }

    #[test]
    fn a_wait_fires_once_and_replays() {
        let mut live = Waits::default();
        let mut log = Vec::new();
        let mut step = |w: &mut Waits, cmd: WaitCmd| -> Result<(), WaitError> {
            let (kind, data) = w.apply(&cmd)?;
            w.replay(kind, &data).unwrap();
            log.push((kind, data));
            Ok(())
        };
        step(&mut live, WaitCmd::Open(wait("w1", agent("a"), vec![timer(NOW), timer(NOW + 5)]))).unwrap();
        step(&mut live, WaitCmd::Open(wait("w2", agent("a"), vec![timer(NOW + 9)]))).unwrap();
        step(&mut live, WaitCmd::Fire { id: "w1".into(), fired: fired(NOW, true) }).unwrap();
        assert_eq!(step(&mut live, WaitCmd::Fire { id: "w1".into(), fired: fired(NOW + 5, true) }).unwrap_err().code, "conflict", "any-of: the first event closes it");
        step(&mut live, WaitCmd::Cancel { id: "w2".into(), by: "user".into(), at: NOW }).unwrap();
        assert_eq!(step(&mut live, WaitCmd::Expire { id: "w2".into(), fired: fired(NOW, true) }).unwrap_err().code, "conflict", "a cancelled wait never wakes");
        let mut replayed = Waits::default();
        for (kind, data) in &log {
            replayed.replay(kind, data).unwrap();
        }
        replayed.replay("wait-future", &json!({})).unwrap();
        assert_eq!(replayed.waits, live.waits);
        assert_eq!(replayed.waits["w1"].status, WaitStatus::Fired);
        assert_eq!(replayed.waits["w2"].cancelled_by.as_deref(), Some("user"));
    }

    #[test]
    fn the_wake_budget_counts_fires_and_auto_starts_in_the_last_hour() {
        let mut w = Waits::default();
        for i in 0..WAKES_PER_HOUR {
            let id = format!("w{i}");
            run(&mut w, WaitCmd::Open(wait(&id, agent("a"), vec![timer(NOW)]))).unwrap();
            let started = i == 0;
            run(&mut w, WaitCmd::Fire { id, fired: Fired { started, ..fired(NOW - HOUR_MS / 2, true) } }).unwrap();
        }
        assert_eq!(w.wakes_in_hour(&agent("a"), NOW), WAKES_PER_HOUR + 1);
        assert_eq!(w.wakes_in_hour(&agent("a"), NOW + HOUR_MS), 0);
        assert_eq!(w.wakes_in_hour(&agent("b"), NOW), 0);
        assert_eq!(wake_plan(WAKES_PER_HOUR - 1, None), WakePlan { wake: true, start: false });
        assert_eq!(wake_plan(WAKES_PER_HOUR, None), WakePlan { wake: false, start: false }, "the 31st fire merges");
        assert_eq!(wake_plan(0, Some(false)), WakePlan { wake: true, start: true });
        assert_eq!(wake_plan(0, Some(true)), WakePlan { wake: true, start: false }, "a crashed agent is never started");
        assert_eq!(wake_plan(WAKES_PER_HOUR - 1, Some(false)), WakePlan { wake: true, start: false }, "no room for the start unit");
    }

    #[test]
    fn merged_events_are_counted_in_the_queued_text() {
        let once = add_more("build exited 1.", 1, "/cli");
        assert_eq!(once, "build exited 1.\n+1 more event: /cli agent waits");
        assert_eq!(add_more(&once, 2, "/cli"), "build exited 1.\n+3 more events: /cli agent waits");
    }

    #[test]
    fn wake_text_carries_the_note_and_stays_short() {
        let w = Wait { note: "fix build".into(), ..wait("w1", agent("a"), vec![timer(NOW)]) };
        assert_eq!(wake_text(&w, "timer."), "timer. Note: fix build");
        let long = wake_text(&w, &"x".repeat(2000));
        assert!(long.len() <= WAKE_MAX_BYTES && long.ends_with("... Note: fix build"), "{}", long.len());
    }

    #[test]
    fn session_summaries() {
        let exited = ProbeSession::Exited { code: Some(101), stopped_by_user: false, last_line: "error[E0308]: mismatched types".into(), cmd: "cargo build".into() };
        let s = session_summary("build@D:/x", false, &exited, None).unwrap();
        assert!(s.starts_with("build `cargo build` exited 101. Last line: error[E0308]"), "{s}");
        assert!(s.contains("/run/logs?cwd=D%3A%2Fx&kind=build&tail=50"), "{s}");
        let unknown = ProbeSession::Exited { code: None, stopped_by_user: false, last_line: String::new(), cmd: String::new() };
        assert_eq!(session_summary("tile1234abcd", false, &unknown, Some(0)).unwrap(), "terminal exited 0.");
        assert_eq!(session_summary("tile1234abcd", false, &unknown, None).unwrap(), "terminal exited with exit code unknown.");
        assert!(session_summary("build@D:/x", false, &ProbeSession::Gone, None).unwrap().contains("exit code unknown"));
        assert_eq!(session_summary("build@D:/x", false, &ProbeSession::Running { ready: Some("x".into()) }, None), None);
        assert_eq!(session_summary("run@D:/x", true, &ProbeSession::Running { ready: Some("Listening on :5173".into()) }, None).unwrap(), "dev run ready: Listening on :5173.");
        assert_eq!(session_summary("run@D:/x", true, &ProbeSession::Running { ready: None }, None), None);
        assert!(session_summary("run@D:/x", true, &exited, None).unwrap().starts_with("dev run exited before ready"));
        assert_eq!(session_summary("build@D:/x", false, &ProbeSession::Unknown, None), None, "no answer from the host is not an exit");
    }

    #[test]
    fn fired_waits_without_their_message_are_repaired_once() {
        let mut w = Waits::default();
        run(&mut w, WaitCmd::Open(wait("w1", agent("a"), vec![timer(NOW)]))).unwrap();
        run(&mut w, WaitCmd::Open(wait("w2", agent("a"), vec![timer(NOW)]))).unwrap();
        run(&mut w, WaitCmd::Open(wait("w3", agent("a"), vec![timer(NOW)]))).unwrap();
        run(&mut w, WaitCmd::Open(wait("w4", agent("a"), vec![timer(NOW)]))).unwrap();
        run(&mut w, WaitCmd::Fire { id: "w1".into(), fired: fired(NOW, true) }).unwrap();
        run(&mut w, WaitCmd::Expire { id: "w2".into(), fired: fired(NOW, true) }).unwrap();
        run(&mut w, WaitCmd::Fire { id: "w3".into(), fired: fired(NOW, false) }).unwrap();
        let sent = ["ew2"];
        let missing: Vec<String> = unsent_wakes(&w, |id| sent.contains(&id)).into_iter().map(|(w, _)| w.id).collect();
        assert_eq!(missing, ["w1"], "w2 has its message, w3 merged into the budget, w4 is open");
    }

    fn serial() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn test_agent(tag: &str, desired: Desired, crashed: bool) -> (String, String) {
        crate::orch::tests::f12_test_home();
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let id = format!("aw{tag}{:08}", nanos % 100_000_000);
        let name = format!("w{tag}-{}", &id[3..]);
        let mut a = crate::agents::reducer::tests::agent(&id, &name);
        a.desired = desired;
        a.crashed = crashed;
        lock().agents.insert(id.clone(), a);
        (id, name)
    }

    fn reply(text: String) -> Value {
        serde_json::from_str(&text).unwrap()
    }

    fn open_direct(waiter: &Party, targets: Vec<Target>) -> String {
        let id = gen_wait_id();
        let w = Wait { id: id.clone(), created_at: unix_ms(), ..wait(&id, waiter.clone(), targets) };
        commit(WaitCmd::Open(w)).unwrap();
        id
    }

    fn far() -> Target {
        timer(unix_ms() + 6 * DAY_MS)
    }

    fn message(id: &str) -> Option<Message> {
        with_mailbox(|mb| mb.messages.get(id).cloned())
    }

    fn status(id: &str) -> WaitStatus {
        with_waits(|w| w.waits[id].status)
    }

    #[test]
    fn a_target_already_true_answers_inline_and_opens_nothing() {
        let _serial = serial();
        let (me, _) = test_agent("i", Desired::Running, false);
        let (_, stopped) = test_agent("j", Desired::Stopped, false);
        let r = reply(handle_wait(&json!({ "on": [format!("agent:{stopped}")], "fromAgent": me })));
        assert_eq!(r["fired"], true, "{r}");
        assert!(r["text"].as_str().unwrap().contains("stopped"), "{r}");
        let past = iso_utc(unix_ms() - 5_000);
        let r = reply(handle_wait(&json!({ "on": [format!("time:{past}")], "fromAgent": me })));
        assert_eq!(r["fired"], true, "{r}");
        assert_eq!(with_waits(|w| w.open_for(&Party::Agent { id: me.clone() }).count()), 0);
    }

    #[test]
    fn registration_rules() {
        let _serial = serial();
        let (me, _) = test_agent("k", Desired::Running, false);
        let (_, other) = test_agent("l", Desired::Running, false);
        let worker = reply(handle_wait(&json!({ "on": ["in:2m"], "worker": true, "fromTile": "t1" })));
        assert_eq!(worker["error"]["code"], "forbidden");
        let first = reply(handle_wait(&json!({ "on": [format!("agent:{other}")], "fromAgent": me, "note": "review done?" })));
        assert!(first["text"].as_str().unwrap().ends_with(END_TURN), "{first}");
        let again = reply(handle_wait(&json!({ "on": [format!("agent:{other}")], "fromAgent": me })));
        assert_eq!(again["existing"], true);
        assert_eq!(again["wait"]["id"], first["wait"]["id"]);
        assert!(first["wait"]["timeoutAt"].as_u64().is_some());
        let timer = reply(handle_wait(&json!({ "on": ["in:5m"], "fromAgent": me })));
        assert!(timer["wait"]["timeoutAt"].is_null(), "a timer needs no timeout: {timer}");
        let self_wait = reply(handle_wait(&json!({ "on": [format!("agent:{}", lock().agents[&me].name)], "fromAgent": me })));
        assert_eq!(self_wait["error"]["code"], "bad_request");
        let long = reply(handle_wait(&json!({ "on": ["in:5m"], "fromAgent": me, "timeoutMs": MAX_TIMEOUT_MS + 1 })));
        assert_eq!(long["error"]["code"], "bad_request");
        let user = reply(handle_wait(&json!({ "on": ["in:3m"] })));
        assert!(!user["text"].as_str().unwrap().contains("End your turn"), "{user}");
    }

    #[test]
    fn a_timeout_wakes_exactly_once_and_a_cancelled_wait_never() {
        let _serial = serial();
        let (me, _) = test_agent("m", Desired::Running, false);
        let (_, other) = test_agent("n", Desired::Running, false);
        let r = reply(handle_wait(&json!({ "on": [format!("agent:{other}")], "fromAgent": me, "timeoutMs": 1_000 })));
        let id = r["wait"]["id"].as_str().unwrap().to_string();
        let r = reply(handle_wait(&json!({ "on": ["in:2m"], "fromAgent": me })));
        let cancelled = r["wait"]["id"].as_str().unwrap().to_string();
        assert_eq!(reply(handle_wait_cancel(&json!({ "id": cancelled, "fromAgent": me })))["cancelled"], 1);
        let later = unix_ms() + 3 * MINUTE_MS;
        tick_once(later, false);
        tick_once(later + 1, false);
        assert_eq!(status(&id), WaitStatus::Expired);
        let wake = message(&wake_id(&id)).expect("one timeout wake");
        assert!(wake.text.starts_with("timed out after"), "{}", wake.text);
        assert!(message(&wake_id(&cancelled)).is_none());
        assert_eq!(status(&cancelled), WaitStatus::Cancelled);
    }

    #[test]
    fn only_the_waiter_or_the_user_cancels_a_wait() {
        let _serial = serial();
        let (me, _) = test_agent("o", Desired::Running, false);
        let (stranger, _) = test_agent("p", Desired::Running, false);
        let id = reply(handle_wait(&json!({ "on": ["in:4m"], "fromAgent": me })))["wait"]["id"].as_str().unwrap().to_string();
        assert_eq!(reply(handle_wait_cancel(&json!({ "id": id, "fromAgent": stranger })))["error"]["code"], "forbidden");
        assert_eq!(reply(handle_wait_cancel(&json!({ "id": id })))["cancelled"], 1);
    }

    #[test]
    fn a_wake_comes_from_the_brain_on_a_fresh_thread() {
        let _serial = serial();
        let (me, _) = test_agent("q", Desired::Running, false);
        let party = Party::Agent { id: me.clone() };
        let id = open_direct(&party, vec![timer(unix_ms() + 2 * MINUTE_MS)]);
        tick_once(unix_ms() + 3 * MINUTE_MS, false);
        let wake = message(&wake_id(&id)).unwrap();
        assert_eq!((wake.from, wake.hops, wake.thread.as_str(), wake.kind.as_deref()), (Party::Brain, 0, id.as_str(), Some(KIND_EVENT)));
        assert_eq!(wake.to, party);
        assert_eq!(crate::agents::messages::inherited_thread(&party), None, "an event starts no thread for the agent");
    }

    #[test]
    fn the_31st_fire_in_an_hour_merges_into_the_queued_event() {
        let _serial = serial();
        let (me, _) = test_agent("r", Desired::Running, false);
        let party = Party::Agent { id: me.clone() };
        let mut ids = Vec::new();
        for _ in 0..=WAKES_PER_HOUR {
            let id = open_direct(&party, vec![far()]);
            assert!(fire(&id, Some(0), "timer.".into(), false));
            ids.push(id);
        }
        let sent = ids.iter().filter(|id| message(&wake_id(id)).is_some()).count();
        assert_eq!(sent, WAKES_PER_HOUR);
        assert!(message(&wake_id(ids.last().unwrap())).is_none());
        let last_sent = message(&wake_id(&ids[WAKES_PER_HOUR - 1])).unwrap();
        assert!(last_sent.text.contains("+1 more event"), "{}", last_sent.text);
    }

    #[test]
    fn a_stopped_waiter_is_started_by_its_event_and_a_crashed_one_is_not() {
        let _serial = serial();
        for (tag, crashed) in [("s", false), ("t", true)] {
            let (me, _) = test_agent(tag, Desired::Stopped, crashed);
            let id = open_direct(&Party::Agent { id: me.clone() }, vec![far()]);
            assert!(fire(&id, Some(0), "timer.".into(), false));
            let agent = lock().agents[&me].clone();
            if crashed {
                assert_eq!(agent.desired, Desired::Stopped);
                assert!(!with_waits(|w| w.waits[&id].fired.as_ref().unwrap().started));
            } else {
                assert_eq!(agent.desired, Desired::Running);
                assert_eq!(agent.started_by.as_deref(), Some(format!("wait {id}").as_str()));
                assert!(with_waits(|w| w.waits[&id].fired.as_ref().unwrap().started));
            }
            assert_eq!(message(&wake_id(&id)).unwrap().status, MsgStatus::Queued);
        }
    }

    #[test]
    fn an_agent_target_fires_after_a_busy_then_idle_turn() {
        let _serial = serial();
        let (me, _) = test_agent("u", Desired::Running, false);
        let (other, other_name) = test_agent("v", Desired::Running, false);
        let id = reply(handle_wait(&json!({ "on": [format!("agent:{other_name}")], "fromAgent": me })))["wait"]["id"].as_str().unwrap().to_string();
        let key = Party::Agent { id: other }.key();
        on_party_status(&key, false, true);
        assert_eq!(status(&id), WaitStatus::Open, "idle before any turn is not an event");
        on_party_status(&key, true, false);
        on_party_status(&key, false, true);
        assert_eq!(status(&id), WaitStatus::Fired);
        assert!(message(&wake_id(&id)).unwrap().text.contains("is idle"));
    }

    #[test]
    fn a_session_exit_fires_with_its_code() {
        let _serial = serial();
        let (me, _) = test_agent("x", Desired::Running, false);
        let session = format!("tile-exit-{}", unix_ms());
        let target = Target::Session { spec: session.clone(), ready: false, session: session.clone(), pid: Some(7) };
        let id = open_direct(&Party::Agent { id: me }, vec![target]);
        tick_once(unix_ms(), false);
        assert_eq!(status(&id), WaitStatus::Open, "an unknown session is not an exit");
        on_session_exit(&session, Some(3));
        assert_eq!(message(&wake_id(&id)).unwrap().text, "terminal exited 3.");
        tick_once(unix_ms(), false);
        assert!(!rt().exit_codes.contains_key(&session), "no open wait watches it any more");
    }

    #[test]
    fn boot_repair_sends_a_missing_wake_once() {
        let _serial = serial();
        let (me, _) = test_agent("y", Desired::Running, false);
        let id = open_direct(&Party::Agent { id: me }, vec![far()]);
        commit(WaitCmd::Fire { id: id.clone(), fired: Fired { target: Some(0), at: unix_ms(), summary: "timer.".into(), wake: true, started: false } }).unwrap();
        assert!(message(&wake_id(&id)).is_none(), "the brain stopped between the two writes");
        repair_unsent_wakes();
        repair_unsent_wakes();
        assert_eq!(with_mailbox(|mb| mb.messages.values().filter(|m| m.id == wake_id(&id)).count()), 1);
    }

    #[test]
    fn a_closed_tile_loses_its_mail_and_waits_and_refuses_new_mail() {
        let _serial = serial();
        let (sender, _) = test_agent("z", Desired::Running, false);
        let tile = Party::Tile { id: format!("tile-closed-{}", unix_ms()) };
        let agent = Party::Agent { id: sender.clone() };
        let base = |id: &str, from: Party, to: Party| Message {
            id: id.into(), from, to, from_name: "x".into(), text: "hi".into(), expects_reply: false, reply_to: None,
            thread: id.into(), hops: 0, at: unix_ms(), seq: 0, status: MsgStatus::Queued, delivered_at: None, replied_by: None,
            reason: None, read: false, notice: false, kind: None, key: None,
        };
        let suffix = unix_ms() % 1_000_000;
        let ask = format!("mca{suffix}");
        let queued = format!("mcb{suffix}");
        commit_mail(MailCmd::Send { message: Message { expects_reply: true, ..base(&ask, tile.clone(), agent.clone()) }, recipient_exists: true }).unwrap();
        commit_mail(MailCmd::Send { message: base(&queued, agent.clone(), tile.clone()), recipient_exists: true }).unwrap();
        let wait_id = open_direct(&tile, vec![far()]);
        let Party::Tile { id: tile_id } = &tile else { unreachable!() };
        crate::agents::on_tile_closed(tile_id);
        let dropped = message(&queued).unwrap();
        assert_eq!((dropped.status, dropped.reason.as_deref()), (MsgStatus::Dropped, Some("tile_closed")));
        let notice = with_mailbox(|mb| mb.messages.values().any(|m| m.notice && m.to == agent && m.text.contains(&queued) && m.text.contains("was closed")));
        assert!(notice, "the agent sender is told");
        assert_eq!(status(&wait_id), WaitStatus::Cancelled);
        let late = reply(crate::agents::messages::handle_reply(&json!({ "to": ask, "text": "answer", "fromAgent": sender })));
        assert_eq!(late["error"]["code"], "tile_closed", "{late}");
        crate::agents::on_tile_opened(tile_id);
        let reopened = reply(crate::agents::messages::handle_reply(&json!({ "to": ask, "text": "answer", "fromAgent": sender })));
        assert_eq!(reopened["ok"], true, "a restarted tile reuses its id: {reopened}");
    }

    fn coordinated_run(tag: &str, desired: Desired, crashed: bool, detached: bool) -> (String, String, String) {
        let (agent, _) = test_agent(tag, desired, crashed);
        let tile = crate::agents::model::tile_id(&agent);
        let run = format!("r-{agent}");
        crate::orch::tests::agent_test_run(&run, &tile, detached);
        (agent, tile, run)
    }

    fn orch_mail(agent: &str) -> Vec<Message> {
        let party = Party::Agent { id: agent.into() };
        with_mailbox(|mb| mb.messages.values().filter(|m| m.to == party && m.kind.as_deref() == Some(crate::agents::mail::KIND_ORCH)).cloned().collect())
    }

    #[test]
    fn an_agent_coordinator_is_woken_by_mail_not_by_typing() {
        let _serial = serial();
        let (agent, tile, run) = coordinated_run("ca", Desired::Running, false, false);
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::recheck_coordinator(&run, &tile);
        let mail = orch_mail(&agent);
        assert_eq!(mail.len(), 1, "{mail:?}");
        assert_eq!(mail[0].from, Party::Brain);
        assert_eq!(crate::agents::envelope::header(&mail[0]), format!("[panorama orch {run}]"));
        assert!(mail[0].text.starts_with("1 new message.") && mail[0].text.contains(&format!("orch check --run-id {run}")), "{}", mail[0].text);
    }

    #[test]
    fn orch_wakes_coalesce_while_queued_and_return_only_for_a_newer_decision() {
        let _serial = serial();
        let (agent, tile, run) = coordinated_run("cb", Desired::Running, false, false);
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::recheck_coordinator(&run, &tile);
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::recheck_coordinator(&run, &tile);
        crate::orch::recheck_coordinator(&run, &tile);
        let mail = orch_mail(&agent);
        assert_eq!(mail.len(), 1, "coalesced while queued: {mail:?}");
        assert!(mail[0].text.starts_with("2 new messages."), "{}", mail[0].text);
        commit_mail(MailCmd::Deliver { ids: vec![mail[0].id.clone()], at: unix_ms() }).unwrap();
        crate::orch::recheck_coordinator(&run, &tile);
        assert_eq!(orch_mail(&agent).len(), 1, "idle without an ack does not wake again");
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::recheck_coordinator(&run, &tile);
        assert_eq!(orch_mail(&agent).len(), 1, "a newer decision within a minute of the last wake waits");
        let top = crate::orch::wait_run_baseline(&run).unwrap();
        assert!(matches!(orch_wake(&agent, &run, top + 1, "x"), OrchWake::Deferred(ms) if ms <= ORCH_MIN_GAP_MS));
    }

    #[test]
    fn a_newer_decision_after_the_gap_wakes_the_coordinator_again() {
        let _serial = serial();
        let (agent, tile, run) = coordinated_run("ck", Desired::Running, false, false);
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::recheck_coordinator(&run, &tile);
        let first = orch_mail(&agent);
        commit_mail(MailCmd::Deliver { ids: vec![first[0].id.clone()], at: unix_ms() - ORCH_MIN_GAP_MS - 1 }).unwrap();
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::recheck_coordinator(&run, &tile);
        assert_eq!(orch_mail(&agent).len(), 2, "a newer decision wakes it");
    }

    #[test]
    fn a_stopped_coordinator_agent_starts_on_a_decision_and_a_crashed_one_does_not() {
        let _serial = serial();
        for (tag, crashed) in [("cc", false), ("cd", true)] {
            let (agent, tile, run) = coordinated_run(tag, Desired::Stopped, crashed, false);
            crate::orch::tests::inject_coordinator_note(&run);
            crate::orch::recheck_coordinator(&run, &tile);
            let a = lock().agents[&agent].clone();
            assert_eq!(orch_mail(&agent).len(), 1);
            if crashed {
                assert_eq!(a.desired, Desired::Stopped);
            } else {
                assert_eq!(a.desired, Desired::Running);
                assert_eq!(a.started_by.as_deref(), Some(format!("orch {run}").as_str()));
            }
        }
    }

    #[test]
    fn run_and_task_waits_fire_on_orch_changes() {
        let _serial = serial();
        let (me, _) = test_agent("ce", Desired::Running, false);
        let (_, _, run) = coordinated_run("cf", Desired::Running, false, false);
        crate::orch::tests::inject_coordinator_note(&run);
        crate::orch::tests::set_test_task(&run, "t1", crate::orch::model::TaskStatus::Pending);
        crate::orch::tests::set_test_task(&run, "t2", crate::orch::model::TaskStatus::Completed);
        let run_wait = reply(handle_wait(&json!({ "on": [format!("run:{run}")], "fromAgent": me })));
        let run_wait = run_wait["wait"]["id"].as_str().expect("an older decision is not the next one").to_string();
        let task_wait = reply(handle_wait(&json!({ "on": [format!("task:{run}/t1")], "fromAgent": me })))["wait"]["id"].as_str().unwrap().to_string();
        let done = reply(handle_wait(&json!({ "on": [format!("task:{run}/t2")], "fromAgent": me })));
        assert_eq!(done["fired"], true, "{done}");
        assert_eq!(reply(handle_wait(&json!({ "on": ["run:r-missing"], "fromAgent": me })))["error"]["code"], "not_found");
        crate::agents::on_orch_commit(&run);
        assert_eq!((status(&run_wait), status(&task_wait)), (WaitStatus::Open, WaitStatus::Open));
        crate::orch::tests::set_test_task(&run, "t1", crate::orch::model::TaskStatus::Failed);
        crate::orch::tests::inject_coordinator_note(&run);
        crate::agents::on_orch_commit(&run);
        assert!(message(&wake_id(&task_wait)).unwrap().text.starts_with("task t1 failed."));
        let wake = message(&wake_id(&run_wait)).unwrap().text;
        assert!(wake.contains("1 new decision(s)") && wake.contains("orch check --run-id"), "{wake}");
    }

    #[test]
    fn a_finished_run_fires_its_wait() {
        let _serial = serial();
        let (me, _) = test_agent("cg", Desired::Running, false);
        let (_, _, run) = coordinated_run("ch", Desired::Running, false, false);
        let id = reply(handle_wait(&json!({ "on": [format!("run:{run}")], "fromAgent": me })))["wait"]["id"].as_str().unwrap().to_string();
        crate::orch::tests::set_test_run_status(&run, crate::orch::model::RunStatus::Completed);
        tick_once(unix_ms(), false);
        assert!(message(&wake_id(&id)).unwrap().text.starts_with(&format!("run {run} completed:")));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn coordinate_needs_claude_and_off_detaches_the_run() {
        let (agent, tile, run) = {
            let _serial = serial();
            coordinated_run("ci", Desired::Stopped, false, false)
        };
        let name = lock().agents[&agent].name.clone();
        lock().agents.get_mut(&agent).unwrap().profile = "kimi".into();
        let refused = reply(crate::agents::handle("coordinate", json!({ "agent": name, "runId": run }).to_string().as_bytes()).await);
        assert_eq!(refused["error"]["code"], "unsupported");
        assert_eq!(crate::orch::runs_coordinated_by(&tile), vec![run.clone()]);
        let off = reply(crate::agents::handle("coordinate", json!({ "agent": name, "runId": run, "on": false }).to_string().as_bytes()).await);
        assert_eq!(off["ok"], true, "{off}");
        assert!(crate::orch::runs_coordinated_by(&tile).is_empty());
        let again = reply(crate::agents::handle("coordinate", json!({ "agent": name, "runId": run, "on": false }).to_string().as_bytes()).await);
        assert_eq!(again["error"]["code"], "conflict");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn deleting_a_coordinator_agent_detaches_its_runs() {
        let (agent, tile, run) = {
            let _serial = serial();
            coordinated_run("cj", Desired::Stopped, false, false)
        };
        let name = lock().agents[&agent].name.clone();
        let deleted = reply(crate::agents::handle("delete", json!({ "name": name }).to_string().as_bytes()).await);
        assert_eq!(deleted["ok"], true, "{deleted}");
        let mut detached = false;
        for _ in 0..40 {
            if crate::orch::runs_coordinated_by(&tile).is_empty() {
                detached = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(detached, "run {run} still points at the deleted agent");
    }

    #[test]
    fn file_changes_are_seen_against_the_registration_stat() {
        let a = FileStat { size: 3, mtime: 10 };
        assert_eq!(file_summary("/x", None, None), None);
        assert_eq!(file_summary("/x", Some(a), Some(a)), None);
        assert_eq!(file_summary("/x", None, Some(a)).unwrap(), "file appeared: /x (3 bytes).");
        assert_eq!(file_summary("/x", Some(a), Some(FileStat { size: 3, mtime: 11 })).unwrap(), "file changed: /x (3 bytes).");
        assert_eq!(file_summary("/x", Some(a), None).unwrap(), "file removed: /x.");
    }

    #[test]
    fn a_file_wait_fires_from_the_poller_cache_not_from_the_tick() {
        let _serial = serial();
        let (me, _) = test_agent("fa", Desired::Running, false);
        let dir = std::env::temp_dir().join(format!("panorama-wait-{}", unix_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.txt");
        let path = file.to_string_lossy().to_string();
        let relative = reply(handle_wait(&json!({ "on": ["file:out.txt"], "fromAgent": me })));
        assert_eq!(relative["error"]["code"], "bad_request");
        let id = reply(handle_wait(&json!({ "on": [format!("file:{path}")], "fromAgent": me })))["wait"]["id"].as_str().unwrap().to_string();
        std::fs::write(&file, "hi").unwrap();
        tick_once(unix_ms(), true);
        assert_eq!(status(&id), WaitStatus::Open, "the tick never stats files itself");
        poll_files_once();
        tick_once(unix_ms(), false);
        assert!(message(&wake_id(&id)).unwrap().text.starts_with("file appeared:"));
        std::fs::remove_dir_all(&dir).ok();
        for remote in ["file://h/s/x", "file:\\\\h\\s\\x", "file:\\\\.\\pipe\\x", "file:\\\\?\\C:\\x", "file:\\\\?\\UNC\\h\\s"] {
            assert_eq!(reply(handle_wait(&json!({ "on": [remote], "fromAgent": me })))["error"]["code"], "bad_request", "{remote}");
        }
    }

    #[test]
    fn only_local_disk_paths_are_watched() {
        for ok in ["C:\\x\\y.txt", "d:/x", "/home/x"] {
            assert!(local_disk_path(ok), "{ok}");
        }
        for bad in ["//host/share/x", "\\\\host\\share\\x", "\\\\.\\pipe\\x", "\\\\?\\C:\\x", "\\\\?\\UNC\\h\\s", "/\\host", "x.txt", "C:x"] {
            assert!(!local_disk_path(bad), "{bad}");
        }
    }

    #[test]
    fn iso_round_trips() {
        for ms in [0, NOW, NOW + 123_456_000] {
            let s = iso_utc(ms);
            assert_eq!(parse_iso(&s), Some(ms / 1000 * 1000), "{s}");
        }
        assert_eq!(parse_duration("2h"), Some(2 * HOUR_MS));
        assert_eq!(parse_duration("1d"), Some(DAY_MS));
        assert_eq!(parse_duration("x"), None);
    }
}
