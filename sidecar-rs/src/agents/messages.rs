use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

use crate::agents::mail::{MailCmd, MailError, Mailbox, Message, MsgStatus, Party};
use crate::agents::model::{Agent, Desired};
use crate::agents::{err_resp, lock, ok_resp, store, text_field, unix_ms};

pub const RATE_PER_MINUTE: usize = 6;
pub const RATE_PER_HOUR: usize = 60;
const MINUTE_MS: u64 = 60_000;
const HOUR_MS: u64 = 3_600_000;
const LIST_DEFAULT: usize = 20;
const LIST_MAX: usize = 200;
const PREVIEW_CHARS: usize = 200;
const ID_ALPHABET: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

#[derive(Default)]
struct MailState {
    mailbox: Mailbox,
    log_seq: u32,
    snap_seq: u32,
}

fn mail() -> &'static Mutex<MailState> {
    static MAIL: OnceLock<Mutex<MailState>> = OnceLock::new();
    MAIL.get_or_init(Default::default)
}

fn mail_lock() -> std::sync::MutexGuard<'static, MailState> {
    mail().lock().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn with_mailbox<T>(f: impl FnOnce(&Mailbox) -> T) -> T {
    f(&mail_lock().mailbox)
}

fn rates() -> &'static Mutex<HashMap<String, RateWindow>> {
    static MAP: OnceLock<Mutex<HashMap<String, RateWindow>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn limited_threads() -> &'static Mutex<HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(Default::default)
}

fn causes() -> &'static Mutex<HashMap<String, Cause>> {
    static MAP: OnceLock<Mutex<HashMap<String, Cause>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

fn guard<T>(m: &'static Mutex<T>) -> std::sync::MutexGuard<'static, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause {
    pub thread: String,
    pub hops: u8,
    pub seen_busy: bool,
}

pub(crate) fn cause_of(key: &str) -> Option<Cause> {
    guard(causes()).get(key).cloned()
}

pub(crate) fn set_cause(key: &str, thread: &str, hops: u8) {
    guard(causes()).insert(key.to_string(), Cause { thread: thread.into(), hops, seen_busy: false });
}

pub(crate) fn put_cause(key: &str, cause: Cause) {
    guard(causes()).insert(key.to_string(), cause);
}

pub(crate) fn clear_cause(key: &str) {
    guard(causes()).remove(key);
}

fn busy_marks() -> &'static Mutex<HashMap<String, u64>> {
    static MAP: OnceLock<Mutex<HashMap<String, u64>>> = OnceLock::new();
    MAP.get_or_init(Default::default)
}

pub(crate) fn last_busy_ms(key: &str) -> Option<u64> {
    guard(busy_marks()).get(key).copied()
}

pub(crate) fn note_status(key: &str, busy: bool, idle: bool) {
    if busy {
        guard(busy_marks()).insert(key.to_string(), unix_ms());
    }
    crate::agents::waits::on_party_status(key, busy, idle);
    crate::agents::routines::on_party_status(key, busy, idle);
    let mut map = guard(causes());
    let Some(cause) = map.get_mut(key) else { return };
    if busy {
        cause.seen_busy = true;
    } else if idle && cause.seen_busy {
        map.remove(key);
    }
}

#[derive(Debug, Default)]
pub struct RateWindow {
    times: VecDeque<u64>,
}

impl RateWindow {
    pub fn check(&self, now: u64, units: usize) -> Result<(), u64> {
        let in_window = |span: u64| self.times.iter().filter(|t| now.saturating_sub(**t) < span).count();
        for (span, cap) in [(MINUTE_MS, RATE_PER_MINUTE), (HOUR_MS, RATE_PER_HOUR)] {
            if in_window(span) + units > cap {
                let oldest = self.times.iter().filter(|t| now.saturating_sub(**t) < span).min().copied().unwrap_or(now);
                return Err((oldest + span).saturating_sub(now));
            }
        }
        Ok(())
    }

    pub fn try_reserve(&mut self, now: u64, units: usize) -> Result<(), u64> {
        self.check(now, units)?;
        self.record(now, units);
        Ok(())
    }

    pub fn release(&mut self, now: u64, units: usize) {
        for _ in 0..units {
            if let Some(pos) = self.times.iter().rposition(|t| *t == now) {
                self.times.remove(pos);
            }
        }
    }

    pub fn record(&mut self, now: u64, units: usize) {
        while self.times.front().is_some_and(|t| now.saturating_sub(*t) >= HOUR_MS) {
            self.times.pop_front();
        }
        self.times.extend(std::iter::repeat_n(now, units));
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartDecision {
    Running,
    AutoStart,
    Crashed,
    LoopGuarded,
    RateLimited,
    StartFailed,
}

pub fn start_decision(agent: &Agent, thread_limited: bool, rate_allows_start: bool) -> StartDecision {
    match agent {
        a if a.desired == Desired::Running => StartDecision::Running,
        a if a.crashed => StartDecision::Crashed,
        _ if thread_limited => StartDecision::LoopGuarded,
        _ if !rate_allows_start => StartDecision::RateLimited,
        _ => StartDecision::AutoStart,
    }
}

fn gen_message_id() -> String {
    let mut bytes = [0u8; 6];
    getrandom::getrandom(&mut bytes).expect("os rng");
    let mut id = String::from("m");
    id.extend(bytes.iter().map(|b| ID_ALPHABET[*b as usize % ID_ALPHABET.len()] as char));
    id
}

pub fn init() {
    let Some(base) = crate::panorama_dir() else { return };
    let path = store::messages_path(&base);
    crate::agents::compact::restore_backup(&path);
    let Ok(bytes) = std::fs::read(&path) else { return };
    let auto_started = auto_started_by_messages(unix_ms());
    let mut state = mail_lock();
    for line in bytes.split(|b| *b == b'\n') {
        let Ok(text) = std::str::from_utf8(line) else { continue };
        let Ok(record) = serde_json::from_str::<store::EventRecord>(text.trim()) else { continue };
        state.log_seq = state.log_seq.max(record.seq);
        if record.kind == "mail-snapshot" {
            state.snap_seq = record.seq;
        }
        if let Err(e) = state.mailbox.replay(&record.kind, &record.data) {
            eprintln!("[agents] skip mail event {}: {e}", record.seq);
        }
    }
    *guard(rates()) = rebuild_rates(&state.mailbox, &auto_started, unix_ms());
    let waiting: Vec<Party> = state.mailbox.messages.values()
        .filter(|m| m.status == MsgStatus::Queued && m.to != Party::User)
        .map(|m| m.to.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    drop(state);
    for party in waiting {
        crate::agents::delivery::kick(&party);
    }
}

pub(crate) fn compact_mail(force: bool) -> crate::agents::compact::Sizes {
    use crate::agents::compact;
    let Some(base) = crate::panorama_dir() else { return compact::Sizes::default() };
    let path = store::messages_path(&base);
    let mut referenced: HashSet<String> = crate::agents::waits::message_ids();
    referenced.extend(crate::agents::routines::message_ids());
    let mut state = mail_lock();
    if state.log_seq == 0 || (!force && !compact::needs_compaction(&path, state.log_seq - state.snap_seq)) {
        return compact::unchanged(&path);
    }
    let data = state.mailbox.snapshot(unix_ms(), &referenced);
    let seq = state.log_seq;
    match compact::compact_log(&path, seq, "mail-snapshot", data.clone()) {
        Ok(sizes) => {
            if let Err(e) = state.mailbox.replay("mail-snapshot", &data) {
                eprintln!("[compact] mail replay: {e}");
            }
            state.snap_seq = seq;
            sizes
        }
        Err(e) => {
            eprintln!("[compact] mail: {e}");
            compact::unchanged(&path)
        }
    }
}

fn parties_of(cmd: &MailCmd, mailbox: &Mailbox) -> Vec<Party> {
    let of = |id: &str| mailbox.messages.get(id).map(|m| vec![m.from.clone(), m.to.clone()]).unwrap_or_default();
    match cmd {
        MailCmd::Send { message, .. } => vec![message.from.clone(), message.to.clone()],
        MailCmd::Deliver { ids, .. } | MailCmd::Read { ids, .. } => ids.iter().flat_map(|id| of(id)).collect(),
        MailCmd::Fail { id, .. } | MailCmd::Drop { id, .. } | MailCmd::Update { id, .. } => of(id),
    }
}

pub(crate) fn commit_mail(cmd: MailCmd) -> Result<Value, MailError> {
    let closed = match &cmd {
        MailCmd::Fail { id, .. } if id.starts_with('u') => Some((id.clone(), true)),
        MailCmd::Drop { id, .. } if id.starts_with('u') => Some((id.clone(), false)),
        _ => None,
    };
    let data = commit_mail_inner(cmd)?;
    if let Some((id, failed)) = closed {
        crate::agents::routines::on_message_closed(&id, failed);
    }
    Ok(data)
}

fn commit_mail_inner(cmd: MailCmd) -> Result<Value, MailError> {
    let io = |message: String| MailError { code: "io", message };
    let (parties, data) = {
        let mut state = mail_lock();
        let (kind, data) = state.mailbox.apply(&cmd)?;
        let base = crate::panorama_dir().ok_or_else(|| io("no panorama dir".into()))?;
        let path = store::messages_path(&base);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| io(e.to_string()))?;
        }
        let seq = state.log_seq + 1;
        let record = store::EventRecord { seq, at: unix_ms(), kind: kind.into(), data: data.clone() };
        store::append_event(&path, &record).map_err(io)?;
        state.log_seq = seq;
        if let Err(e) = state.mailbox.replay(kind, &data) {
            eprintln!("[agents] mail replay after commit {kind}: {e}");
        }
        let mut parties = parties_of(&cmd, &state.mailbox);
        parties.sort();
        parties.dedup();
        (parties, data)
    };
    crate::ui::broadcast_agents_mail(&parties.iter().map(Party::key).collect::<Vec<_>>());
    for agent in parties.iter().filter_map(Party::agent_id) {
        crate::ui::broadcast_agents_changed(agent);
    }
    Ok(data)
}

fn mail_err(e: MailError) -> String {
    err_resp(e.code, &e.message)
}

pub(crate) fn sender_from(v: &Value) -> (Party, String) {
    let agents = lock();
    if let Some(agent) = text_field(v, "fromAgent").and_then(|id| agents.agents.get(&id).cloned()) {
        return (Party::Agent { id: agent.id }, agent.name);
    }
    drop(agents);
    if let Some(tile) = text_field(v, "fromTile").filter(|t| crate::lookup_live(t).is_some()) {
        let short: String = tile.chars().take(8).collect();
        return (Party::Tile { id: tile }, format!("terminal {short}"));
    }
    (Party::User, "user".into())
}

fn party_name(party: &Party) -> String {
    match party {
        Party::Agent { id } => lock().agents.get(id).map(|a| a.name.clone()).unwrap_or_else(|| id.clone()),
        Party::Tile { id } => format!("terminal {}", id.chars().take(8).collect::<String>()),
        Party::User => "user".into(),
        Party::Brain => "panorama".into(),
    }
}

fn resolve_recipient(to: &str) -> Option<(Party, Option<Agent>)> {
    if to.eq_ignore_ascii_case("user") {
        return Some((Party::User, None));
    }
    let agent = lock().resolve(to).cloned()?;
    Some((Party::Agent { id: agent.id.clone() }, Some(agent)))
}

struct Outgoing {
    from: Party,
    from_name: String,
    to: Party,
    text: String,
    expects_reply: bool,
    reply_to: Option<String>,
    thread: Option<(String, u8)>,
}

fn guard_hit(code: &str) -> bool {
    matches!(code, "rejected" | "pair_limit" | "rate_limited")
}

fn reserve(sender: &str, now: u64, units: usize) -> Result<(), u64> {
    guard(rates()).entry(sender.to_string()).or_default().try_reserve(now, units)
}

fn release(sender: &str, now: u64, units: usize) {
    if let Some(window) = guard(rates()).get_mut(sender) {
        window.release(now, units);
    }
}

fn auto_started_by_messages(now: u64) -> HashSet<String> {
    lock().agents.values()
        .filter(|a| a.last_started_at.is_some_and(|at| now.saturating_sub(at) < HOUR_MS))
        .filter_map(|a| a.started_by.as_deref()?.strip_prefix("message ").map(str::to_string))
        .collect()
}

pub(crate) fn rebuild_rates(mailbox: &Mailbox, auto_started: &HashSet<String>, now: u64) -> HashMap<String, RateWindow> {
    let mut windows: HashMap<String, RateWindow> = HashMap::new();
    let mut recent: Vec<&Message> = mailbox.messages.values()
        .filter(|m| !m.notice && now.saturating_sub(m.at) < HOUR_MS)
        .collect();
    recent.sort_by_key(|m| m.at);
    for m in recent {
        let units = if auto_started.contains(&m.id) { 2 } else { 1 };
        windows.entry(m.from.key()).or_default().record(m.at, units);
    }
    windows
}

fn post(out: Outgoing, recipient: Option<Agent>) -> Result<(Message, StartDecision), String> {
    let now = unix_ms();
    let id = gen_message_id();
    let (thread, hops) = out.thread.clone().unwrap_or_else(|| (id.clone(), 0));
    let sender_key = out.from.key();
    if let Party::Tile { id } = &out.to {
        if crate::agents::tile_closed(id) {
            return Err(err_resp("tile_closed", &format!("terminal {} was closed", id.chars().take(8).collect::<String>())));
        }
    }
    let thread_limited = guard(limited_threads()).contains(&thread);
    let mark_limited = |code: &str| {
        if guard_hit(code) {
            guard(limited_threads()).insert(thread.clone());
        }
    };
    if let Err(retry_ms) = reserve(&sender_key, now, 1) {
        mark_limited("rate_limited");
        return Err(err_resp("rate_limited", &format!("send limit {RATE_PER_MINUTE}/min, {RATE_PER_HOUR}/hour; retry in {}s", retry_ms.div_ceil(1000))));
    }
    let message = Message {
        id, from: out.from, to: out.to, from_name: out.from_name, text: out.text, expects_reply: out.expects_reply,
        reply_to: out.reply_to, thread: thread.clone(), hops, at: now, seq: 0, status: MsgStatus::Queued,
        delivered_at: None, replied_by: None, reason: None, read: false, notice: false, kind: None, key: None,
    };
    if let Err(e) = commit_mail(MailCmd::Send { message: message.clone(), recipient_exists: true }) {
        release(&sender_key, now, 1);
        mark_limited(e.code);
        return Err(mail_err(e));
    }
    let wanted = recipient.as_ref()
        .map(|agent| start_decision(agent, thread_limited, true))
        .unwrap_or(StartDecision::Running);
    let decision = match wanted {
        StartDecision::AutoStart if reserve(&sender_key, now, 1).is_err() => StartDecision::RateLimited,
        other => other,
    };
    let decision = match (&decision, &recipient) {
        (StartDecision::AutoStart, Some(agent)) => {
            let started = crate::agents::start_agent(&agent.id, Some(format!("message {}", message.id)));
            start_outcome(started.map_err(|e| e.message), || release(&sender_key, now, 1))
        }
        _ => decision,
    };
    if message.to == Party::User {
        if let Err(e) = commit_mail(MailCmd::Deliver { ids: vec![message.id.clone()], at: now }) {
            eprintln!("[agents] panel delivery of {}: {}", message.id, e.message);
        }
    }
    crate::agents::delivery::kick_for(&message);
    Ok((message, decision))
}

fn reply_hint(from: &Party) -> &'static str {
    match from {
        Party::User => " The reply will appear in Messages.",
        _ => " End your turn now; the reply will be typed here when it arrives.",
    }
}

fn recipient_note(decision: &StartDecision) -> &'static str {
    match decision {
        StartDecision::Running => "",
        StartDecision::AutoStart => " It was stopped and is starting; delivery follows when it is ready.",
        StartDecision::Crashed => " It is crashed and will not start by itself; the message waits until the user starts it.",
        StartDecision::LoopGuarded => " It is stopped and this thread hit a loop limit, so it was not started; the message waits.",
        StartDecision::RateLimited => " It is stopped and the send limit leaves no room to start it; the message waits.",
        StartDecision::StartFailed => " It is stopped and could not be started; the message waits until it starts.",
    }
}

fn decision_name(decision: &StartDecision) -> &'static str {
    match decision {
        StartDecision::Running => "running",
        StartDecision::AutoStart => "auto_started",
        StartDecision::Crashed => "crashed",
        StartDecision::LoopGuarded => "stopped_loop_guarded",
        StartDecision::RateLimited => "stopped_rate_limited",
        StartDecision::StartFailed => "stopped_start_failed",
    }
}

pub(crate) fn send_from_agent(from_id: &str, to_id: &str, text: &str, thread: &str) -> Result<(), String> {
    let from = Party::Agent { id: from_id.to_string() };
    let from_name = party_name(&from);
    let recipient = lock().agents.get(to_id).cloned();
    let out = Outgoing {
        from, from_name, to: Party::Agent { id: to_id.to_string() }, text: text.to_string(), expects_reply: false, reply_to: None,
        thread: Some((thread.to_string(), 0)),
    };
    post(out, recipient).map(|_| ()).map_err(|resp| serde_json::from_str::<Value>(&resp).ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_string)).unwrap_or(resp))
}

pub(crate) fn inherited_thread(from: &Party) -> Option<(String, u8)> {
    cause_of(&from.key()).map(|c| (c.thread, c.hops.saturating_add(1)))
}

pub(crate) fn start_outcome(started: Result<(), String>, refund: impl FnOnce()) -> StartDecision {
    match started {
        Ok(()) => StartDecision::AutoStart,
        Err(e) => {
            eprintln!("[agents] auto-start failed: {e}");
            refund();
            StartDecision::StartFailed
        }
    }
}

pub fn handle_send(v: &Value) -> String {
    let to = text_field(v, "to").unwrap_or_default();
    let text = text_field(v, "text").unwrap_or_default();
    let expects_reply = v.get("expectsReply").and_then(Value::as_bool) == Some(true);
    if expects_reply && v.get("worker").and_then(Value::as_bool) == Some(true) {
        return err_resp("bad_request", "workers cannot wait for replies");
    }
    let Some((to_party, recipient)) = resolve_recipient(&to) else {
        return err_resp("not_found", &format!("no agent named {to}"));
    };
    let (from, from_name) = sender_from(v);
    let thread = inherited_thread(&from);
    let to_name = party_name(&to_party);
    let out = Outgoing { from, from_name, to: to_party, text, expects_reply, reply_to: None, thread };
    match post(out, recipient) {
        Ok((message, decision)) => {
            let ahead = with_mailbox(|mb| mb.queue_for(&message.to).len().saturating_sub(1));
            let mut line = format!("Sent {} to {to_name}.", message.id);
            if expects_reply {
                line.push_str(reply_hint(&message.from));
            }
            line.push_str(recipient_note(&decision));
            ok_resp(json!({ "message": message, "recipientStatus": decision_name(&decision), "queuedAhead": ahead, "text": line }))
        }
        Err(resp) => resp,
    }
}

pub fn handle_reply(v: &Value) -> String {
    let parent_id = text_field(v, "to").unwrap_or_default();
    let text = text_field(v, "text").unwrap_or_default();
    let Some(parent) = with_mailbox(|mb| mb.messages.get(&parent_id).cloned()) else {
        return err_resp("not_found", &format!("message {parent_id} not found"));
    };
    let (from, from_name) = sender_from(v);
    let recipient = parent.from.agent_id().and_then(|id| lock().agents.get(id).cloned());
    let to_name = party_name(&parent.from);
    let out = Outgoing {
        from, from_name, to: parent.from.clone(), text, expects_reply: false, reply_to: Some(parent.id.clone()),
        thread: Some((parent.thread.clone(), parent.hops.saturating_add(1))),
    };
    match post(out, recipient) {
        Ok((message, decision)) => {
            let line = format!("Replied {} to {to_name}.{}", message.id, recipient_note(&decision));
            ok_resp(json!({ "message": message, "recipientStatus": decision_name(&decision), "text": line }))
        }
        Err(resp) => resp,
    }
}

fn compact(m: &Message) -> Value {
    let mut v = serde_json::to_value(m).unwrap_or_default();
    let preview: String = m.text.chars().take(PREVIEW_CHARS).collect();
    v["text"] = preview.into();
    v["truncated"] = (m.text.chars().count() > PREVIEW_CHARS).into();
    v["fromLabel"] = m.from_name.clone().into();
    v["toLabel"] = party_name(&m.to).into();
    if let Some(id) = m.key.as_deref().and_then(|k| k.strip_prefix("routine:")).and_then(|k| k.split(':').next()) {
        v["routineName"] = crate::agents::routines::name_of(id).into();
    }
    v
}

pub fn handle_messages(v: &Value) -> String {
    let party = match (text_field(v, "agent"), text_field(v, "party")) {
        (Some(key), _) => match lock().resolve(&key) {
            Some(agent) => Some(Party::Agent { id: agent.id.clone() }),
            None => return err_resp("not_found", "agent not found"),
        },
        (None, Some(p)) if p == "user" => Some(Party::User),
        _ => None,
    };
    let thread = text_field(v, "thread");
    let status = text_field(v, "status");
    let pending = v.get("pending").and_then(Value::as_bool) == Some(true);
    let limit = v.get("limit").and_then(Value::as_u64).map_or(LIST_DEFAULT, |n| (n as usize).clamp(1, LIST_MAX));
    let list: Vec<Message> = with_mailbox(|mb| {
        let mut all: Vec<&Message> = mb.messages.values()
            .filter(|m| party.as_ref().is_none_or(|p| m.from == *p || m.to == *p))
            .filter(|m| thread.as_ref().is_none_or(|t| m.thread == *t))
            .filter(|m| status.as_ref().is_none_or(|s| serde_json::to_value(m.status).ok().and_then(|x| x.as_str().map(str::to_string)).as_deref() == Some(s)))
            .filter(|m| !pending || m.status == MsgStatus::Queued || (m.expects_reply && m.replied_by.is_none()))
            .collect();
        all.sort_by_key(|m| std::cmp::Reverse(m.seq));
        all.into_iter().take(limit).cloned().collect()
    });
    let unread = with_mailbox(|mb| mb.unread_for(&Party::User));
    ok_resp(json!({ "messages": list.iter().map(compact).collect::<Vec<_>>(), "userUnread": unread }))
}

pub fn handle_read(v: &Value) -> String {
    let id = text_field(v, "id").unwrap_or_default();
    match with_mailbox(|mb| mb.messages.get(&id).cloned()) {
        Some(m) => {
            let mut full = serde_json::to_value(&m).unwrap_or_default();
            full["toLabel"] = party_name(&m.to).into();
            ok_resp(json!({ "message": full }))
        }
        None => err_resp("not_found", &format!("message {id} not found")),
    }
}

pub fn handle_mark_read(v: &Value) -> String {
    let all = v.get("all").and_then(Value::as_bool) == Some(true);
    let upto = v.get("uptoSeq").and_then(Value::as_u64).map_or(u32::MAX, |s| s.min(u32::MAX as u64) as u32);
    let ids: Vec<String> = v["ids"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
    let unread: Vec<String> = with_mailbox(|mb| mb.messages.values()
        .filter(|m| !m.read && m.to == Party::User && (all && m.seq <= upto || ids.contains(&m.id)))
        .map(|m| m.id.clone())
        .collect());
    if unread.is_empty() {
        return ok_resp(json!({}));
    }
    match commit_mail(MailCmd::Read { ids: unread, at: unix_ms() }) {
        Ok(_) => ok_resp(json!({})),
        Err(e) => mail_err(e),
    }
}

pub(crate) fn mail_counts(agent_id: &str) -> (usize, usize) {
    let party = Party::Agent { id: agent_id.to_string() };
    with_mailbox(|mb| (mb.queue_for(&party).len(), mb.awaiting_replies(&party)))
}

pub(crate) fn on_agent_deleted(agent_id: &str, name: &str) {
    drop_queued_for(&Party::Agent { id: agent_id.to_string() }, "recipient_deleted", &format!("agent {name} was deleted"), false);
}

pub(crate) fn drop_queued_for(party: &Party, reason: &str, why: &str, notify_any: bool) {
    let party = party.clone();
    let queued: Vec<Message> = with_mailbox(|mb| mb.queue_for(&party).into_iter().cloned().collect());
    for m in queued {
        if let Err(e) = commit_mail(MailCmd::Drop { id: m.id.clone(), at: unix_ms(), reason: reason.into() }) {
            eprintln!("[agents] drop {}: {}", m.id, e.message);
            continue;
        }
        if (m.expects_reply || notify_any) && !matches!(m.from, Party::User | Party::Brain) {
            let notice = Message {
                id: gen_message_id(), from: Party::User, to: m.from.clone(), from_name: "panorama".into(),
                text: format!("{} was dropped: {why}.", m.id), expects_reply: false, reply_to: None,
                thread: m.thread.clone(), hops: m.hops, at: unix_ms(), seq: 0, status: MsgStatus::Queued,
                delivered_at: None, replied_by: None, reason: None, read: false, notice: true, kind: None, key: None,
            };
            let to = notice.to.clone();
            match commit_mail(MailCmd::Send { message: notice, recipient_exists: true }) {
                Ok(_) => crate::agents::delivery::kick(&to),
                Err(e) => eprintln!("[agents] notice for {}: {}", m.id, e.message),
            }
        }
    }
    guard(causes()).remove(&party.key());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::reducer::tests::agent;

    #[test]
    fn rate_window_allows_six_per_minute_and_sixty_per_hour() {
        let mut w = RateWindow::default();
        let t0 = 10 * HOUR_MS;
        for i in 0..RATE_PER_MINUTE as u64 {
            assert!(w.check(t0 + i, 1).is_ok());
            w.record(t0 + i, 1);
        }
        let retry = w.check(t0 + 10, 1).unwrap_err();
        assert!(retry > 0 && retry <= MINUTE_MS, "{retry}");
        assert!(w.check(t0 + MINUTE_MS, 1).is_ok(), "the minute window slides");
        let mut h = RateWindow::default();
        for i in 0..RATE_PER_HOUR as u64 {
            h.record(t0 + i * 59_000, 1);
        }
        assert!(h.check(t0 + 60 * 59_000, 1).is_err(), "the hour cap holds");
        assert!(h.check(t0 + HOUR_MS + 59_000, 1).is_ok());
    }

    #[test]
    fn an_auto_start_costs_two_units() {
        let mut w = RateWindow::default();
        for i in 0..(RATE_PER_MINUTE as u64 - 1) {
            w.record(i, 1);
        }
        assert!(w.check(10, 1).is_ok());
        assert!(w.check(10, 2).is_err());
    }

    #[test]
    fn start_decision_rules() {
        let mut a = agent("a1", "rev");
        assert_eq!(start_decision(&a, false, true), StartDecision::AutoStart);
        assert_eq!(start_decision(&a, true, true), StartDecision::LoopGuarded);
        assert_eq!(start_decision(&a, false, false), StartDecision::RateLimited);
        a.crashed = true;
        assert_eq!(start_decision(&a, false, true), StartDecision::Crashed);
        a.crashed = false;
        a.desired = Desired::Running;
        assert_eq!(start_decision(&a, true, false), StartDecision::Running);
    }

    fn reply_of(text: String) -> Value {
        serde_json::from_str(&text).unwrap()
    }

    fn make_agent(tag: &str, desired: Desired, crashed: bool) -> (String, String) {
        crate::orch::tests::f12_test_home();
        let id = format!("a{}{:06}", tag, unix_ms() % 1_000_000);
        let name = format!("{tag}-{}", &id[2..]);
        let mut a = agent(&id, &name);
        a.desired = desired;
        a.crashed = crashed;
        lock().agents.insert(id.clone(), a);
        (id, name)
    }

    #[test]
    fn send_auto_starts_a_stopped_agent_and_records_who_started_it() {
        let (from, _) = make_agent("s", Desired::Running, false);
        let (to, to_name) = make_agent("t", Desired::Stopped, false);
        let r = reply_of(handle_send(&json!({ "to": to_name, "text": "hi", "fromAgent": from })));
        assert_eq!(r["recipientStatus"], "auto_started", "{r}");
        let id = r["message"]["id"].as_str().unwrap().to_string();
        let started = lock().agents[&to].clone();
        assert_eq!(started.desired, Desired::Running);
        assert_eq!(started.started_by.as_deref(), Some(format!("message {id}").as_str()));
        assert_eq!(r["message"]["from"], json!({ "kind": "agent", "id": from }));
        assert!(r["text"].as_str().unwrap().contains("starting"), "{r}");
    }

    #[test]
    fn an_auto_start_counts_in_the_senders_rate_window() {
        let (from, _) = make_agent("r", Desired::Running, false);
        let (_, busy_name) = make_agent("b", Desired::Running, false);
        let (_, stopped_name) = make_agent("q", Desired::Stopped, false);
        let (_, other_stopped) = make_agent("v", Desired::Stopped, false);
        for i in 0..(RATE_PER_MINUTE - 3) {
            let r = reply_of(handle_send(&json!({ "to": busy_name, "text": format!("n{i}"), "fromAgent": from })));
            assert_eq!(r["ok"], true, "{r}");
        }
        let r = reply_of(handle_send(&json!({ "to": stopped_name, "text": "wake", "fromAgent": from })));
        assert_eq!(r["recipientStatus"], "auto_started", "{r}");
        let r = reply_of(handle_send(&json!({ "to": other_stopped, "text": "wake too", "fromAgent": from })));
        assert_eq!(r["recipientStatus"], "stopped_rate_limited", "one unit left, an auto-start needs two: {r}");
        let r = reply_of(handle_send(&json!({ "to": busy_name, "text": "over", "fromAgent": from })));
        assert_eq!(r["error"]["code"], "rate_limited", "{r}");
    }
    fn inbox_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn mail_to_the_user_is_delivered_to_the_panel_at_once() {
        let _inbox = inbox_lock();
        let (from, _) = make_agent("u", Desired::Running, false);
        let r = reply_of(handle_send(&json!({ "to": "user", "text": "done", "fromAgent": from })));
        let id = r["message"]["id"].as_str().unwrap().to_string();
        with_mailbox(|mb| {
            assert_eq!(mb.messages[&id].status, MsgStatus::Delivered);
            assert!(!mb.messages[&id].read);
        });
    }
    #[test]
    fn mark_read_ignores_messages_not_addressed_to_the_user() {
        let _inbox = inbox_lock();
        let (agent_id, agent_name) = make_agent("h", Desired::Running, false);
        let to_user = reply_of(handle_send(&json!({ "to": "user", "text": "for you", "fromAgent": agent_id })));
        let from_user = reply_of(handle_send(&json!({ "to": agent_name, "text": "from you", "expectsReply": true })));
        assert!(from_user["text"].as_str().unwrap().contains("appear in Messages"), "{from_user}");
        let ids = [to_user["message"]["id"].clone(), from_user["message"]["id"].clone()];
        let r = reply_of(handle_mark_read(&json!({ "ids": ids })));
        assert_eq!(r["ok"], true, "{r}");
        let id = to_user["message"]["id"].as_str().unwrap().to_string();
        with_mailbox(|mb| assert!(mb.messages[&id].read));
    }
    #[test]
    fn concurrent_sends_at_the_rate_edge_never_exceed_the_limit() {
        let (from, _) = make_agent("z", Desired::Running, false);
        let (_, to_name) = make_agent("y", Desired::Running, false);
        let gate = std::sync::Arc::new(std::sync::Barrier::new(12));
        let handles: Vec<_> = (0..12).map(|i| {
            let (gate, from, to_name) = (gate.clone(), from.clone(), to_name.clone());
            std::thread::spawn(move || {
                gate.wait();
                reply_of(handle_send(&json!({ "to": to_name, "text": format!("n{i}"), "fromAgent": from })))["ok"] == true
            })
        }).collect();
        let sent = handles.into_iter().map(|h| h.join().unwrap()).filter(|ok| *ok).count();
        assert_eq!(sent, RATE_PER_MINUTE);
    }

    #[test]
    fn a_duplicate_reply_does_not_mark_the_thread_as_a_loop() {
        let (asker, asker_name) = make_agent("e", Desired::Running, false);
        let (helper, _) = make_agent("f", Desired::Running, false);
        let (_, sleeper) = make_agent("g", Desired::Stopped, false);
        let helper_name = lock().agents[&helper].name.clone();
        let ask = reply_of(handle_send(&json!({ "to": helper_name, "text": "q", "expectsReply": true, "fromAgent": asker })));
        let ask_id = ask["message"]["id"].as_str().unwrap().to_string();
        let thread = ask["message"]["thread"].as_str().unwrap().to_string();
        assert_eq!(reply_of(handle_reply(&json!({ "to": ask_id, "text": "a", "fromAgent": helper })))["ok"], true);
        let again = reply_of(handle_reply(&json!({ "to": ask_id, "text": "a again", "fromAgent": helper })));
        assert_eq!(again["error"]["code"], "conflict");
        set_cause(&Party::Agent { id: helper.clone() }.key(), &thread, 1);
        let wake = reply_of(handle_send(&json!({ "to": sleeper, "text": "join us", "fromAgent": helper })));
        assert_eq!(wake["recipientStatus"], "auto_started", "{wake}");
        let _ = asker_name;
    }
    #[test]
    fn the_rate_window_is_rebuilt_from_the_log() {
        let now = 50 * HOUR_MS;
        let mut mb = Mailbox::default();
        for i in 0..RATE_PER_MINUTE {
            let m = Message { at: now - 1_000 - i as u64, ..crate::agents::mail::tests::msg(&format!("r{i}"), Party::Agent { id: "ax".into() }, Party::User) };
            mb.messages.insert(m.id.clone(), m);
        }
        let old = Message { at: now - 2 * HOUR_MS, ..crate::agents::mail::tests::msg("old", Party::Agent { id: "ay".into() }, Party::User) };
        mb.messages.insert(old.id.clone(), old);
        let windows = rebuild_rates(&mb, &HashSet::new(), now);
        assert!(windows["agent:ax"].check(now, 1).is_err());
        assert!(!windows.contains_key("agent:ay"), "older than an hour");
    }

    #[test]
    fn the_rebuilt_window_counts_the_extra_unit_of_an_auto_start() {
        let now = 50 * HOUR_MS;
        let mut mb = Mailbox::default();
        for i in 0..3 {
            let m = Message { at: now - 1_000 - i as u64, ..crate::agents::mail::tests::msg(&format!("s{i}"), Party::Agent { id: "az".into() }, Party::User) };
            mb.messages.insert(m.id.clone(), m);
        }
        let windows = rebuild_rates(&mb, &HashSet::from(["s1".to_string()]), now);
        assert!(windows["agent:az"].check(now, 2).is_ok());
        assert!(windows["agent:az"].check(now, 3).is_err(), "3 messages + 1 auto-start = 4 of 6 units");
    }

    #[test]
    fn a_refund_in_the_same_millisecond_leaves_the_other_send_counted() {
        let mut window = RateWindow::default();
        let now = 10 * HOUR_MS;
        window.try_reserve(now, 1).unwrap();
        window.try_reserve(now, 1).unwrap();
        window.release(now, 1);
        assert_eq!(window.times.len(), 1, "units with the same timestamp are interchangeable");
        for _ in 0..RATE_PER_MINUTE - 1 {
            window.try_reserve(now, 1).unwrap();
        }
        assert!(window.try_reserve(now, 1).is_err());
    }

    #[test]
    fn a_failed_auto_start_is_reported_and_refunded() {
        let mut refunded = false;
        assert_eq!(start_outcome(Err("io".into()), || refunded = true), StartDecision::StartFailed);
        assert!(refunded);
        let mut refunded = false;
        assert_eq!(start_outcome(Ok(()), || refunded = true), StartDecision::AutoStart);
        assert!(!refunded);
        assert_eq!(decision_name(&StartDecision::StartFailed), "stopped_start_failed");
    }

    #[test]
    fn mark_read_all_clears_every_unread_message_to_the_user() {
        let _inbox = inbox_lock();
        let (from, _) = make_agent("k", Desired::Running, false);
        for i in 0..3 {
            reply_of(handle_send(&json!({ "to": "user", "text": format!("note {i}"), "fromAgent": from })));
        }
        assert_eq!(reply_of(handle_mark_read(&json!({ "all": true })))["ok"], true);
        assert_eq!(with_mailbox(|mb| mb.unread_for(&Party::User)), 0);
    }

    #[test]
    fn mark_read_all_stops_at_the_last_message_shown() {
        let _inbox = inbox_lock();
        let (from, _) = make_agent("u", Desired::Running, false);
        let shown = reply_of(handle_send(&json!({ "to": "user", "text": "seen", "fromAgent": from })));
        let late = reply_of(handle_send(&json!({ "to": "user", "text": "arrived after the list", "fromAgent": from })));
        let seq = with_mailbox(|mb| mb.messages[shown["message"]["id"].as_str().unwrap()].seq);
        assert_eq!(reply_of(handle_mark_read(&json!({ "all": true, "uptoSeq": seq })))["ok"], true);
        let read = |r: &Value| with_mailbox(|mb| mb.messages[r["message"]["id"].as_str().unwrap()].read);
        assert!(read(&shown));
        assert!(!read(&late), "a message the list never showed stays unread");
    }
    #[test]
    fn send_never_auto_starts_a_crashed_agent() {
        let (from, _) = make_agent("c", Desired::Running, false);
        let (to, to_name) = make_agent("d", Desired::Stopped, true);
        let r = reply_of(handle_send(&json!({ "to": to_name, "text": "hi", "fromAgent": from })));
        assert_eq!(r["recipientStatus"], "crashed", "{r}");
        assert_eq!(lock().agents[&to].desired, Desired::Stopped);
        assert!(r["text"].as_str().unwrap().contains("will not start by itself"), "{r}");
        assert_eq!(mail_counts(&to).0, 1, "the message stays queued");
    }

    #[test]
    fn a_message_from_a_loop_stopped_by_the_guards_never_auto_starts() {
        let (from, _) = make_agent("l", Desired::Running, false);
        let (to, to_name) = make_agent("m", Desired::Stopped, false);
        let thread = format!("mloop{}", &from[2..]);
        set_cause(&Party::Agent { id: from.clone() }.key(), &thread, crate::agents::mail::MAX_HOPS);
        let refused = reply_of(handle_send(&json!({ "to": to_name, "text": "again", "fromAgent": from })));
        assert_eq!(refused["error"]["code"], "rejected", "{refused}");
        assert_eq!(lock().agents[&to].desired, Desired::Stopped);
        set_cause(&Party::Agent { id: from.clone() }.key(), &thread, 1);
        let later = reply_of(handle_send(&json!({ "to": to_name, "text": "later", "fromAgent": from })));
        assert_eq!(later["recipientStatus"], "stopped_loop_guarded", "{later}");
        assert_eq!(lock().agents[&to].desired, Desired::Stopped);
        guard(causes()).remove(&Party::Agent { id: from.clone() }.key());
    }

    #[test]
    fn workers_cannot_wait_and_unknown_senders_become_user() {
        let (to, to_name) = make_agent("w", Desired::Running, false);
        let r = reply_of(handle_send(&json!({ "to": to_name, "text": "x", "expectsReply": true, "worker": true, "fromTile": "nope" })));
        assert_eq!(r["error"]["code"], "bad_request");
        let r = reply_of(handle_send(&json!({ "to": to_name, "text": "x", "fromTile": "not-a-live-tile" })));
        assert_eq!(r["message"]["from"], json!({ "kind": "user" }), "{r}");
        assert_eq!(r["recipientStatus"], "running");
        let _ = to;
    }

    #[test]
    fn deleting_a_recipient_drops_its_queue_and_notifies_waiting_senders() {
        let (from, _) = make_agent("x", Desired::Running, false);
        let (to, to_name) = make_agent("y", Desired::Running, false);
        let ask = reply_of(handle_send(&json!({ "to": to_name, "text": "q", "expectsReply": true, "fromAgent": from })));
        let plain = reply_of(handle_send(&json!({ "to": to_name, "text": "fyi", "fromAgent": from })));
        on_agent_deleted(&to, &to_name);
        let (ask_id, plain_id) = (ask["message"]["id"].as_str().unwrap(), plain["message"]["id"].as_str().unwrap());
        with_mailbox(|mb| {
            assert_eq!(mb.messages[ask_id].status, MsgStatus::Dropped);
            assert_eq!(mb.messages[plain_id].status, MsgStatus::Dropped);
            let notices: Vec<&Message> = mb.queue_for(&Party::Agent { id: from.clone() }).into_iter().filter(|m| m.notice).collect();
            assert_eq!(notices.len(), 1, "only the expects-reply message produces a notice");
            assert!(notices[0].text.contains(ask_id) && notices[0].text.contains("was deleted"));
        });
    }
    #[test]
    fn every_busy_is_stamped_even_without_a_cause() {
        let before = unix_ms();
        note_status("abusy01", false, true);
        assert_eq!(last_busy_ms("abusy01"), None);
        note_status("abusy01", true, false);
        assert!(last_busy_ms("abusy01").is_some_and(|at| at >= before));
    }

    #[test]
    fn cause_clears_only_after_busy_then_idle() {
        set_cause("acause01", "m1", 2);
        note_status("acause01", false, true);
        assert!(cause_of("acause01").is_some(), "an idle before the turn starts keeps the cause");
        note_status("acause01", true, false);
        assert!(cause_of("acause01").is_some());
        note_status("acause01", false, true);
        assert!(cause_of("acause01").is_none());
    }
}
